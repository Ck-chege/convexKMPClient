use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(all(debug_assertions, target_os = "android"))]
use android_logger::{Config, init_once};
#[cfg(all(debug_assertions, target_os = "android"))]
use log::LevelFilter;
use async_once_cell::OnceCell;
use convex::{ConvexClient, FunctionResult, Value};
use futures::{
    channel::oneshot::{self, Sender},
    pin_mut,
    select_biased,
    FutureExt,
    StreamExt,
};
use log::debug;
use parking_lot::Mutex;
use tokio::task::JoinHandle;

#[derive(Debug, thiserror::Error, Clone)]
enum ClientError {
    #[error("InternalError: {msg}")]
    InternalError { msg: String },
    #[error("ConvexError: {data}")]
    ConvexError { data: String },
    #[error("ServerError: {msg}")]
    ServerError { msg: String },
    #[error("InvalidContext: {msg}")]
    InvalidContext { msg: String },
}

impl From<anyhow::Error> for ClientError {
    fn from(value: anyhow::Error) -> Self {
        Self::InternalError {
            msg: value.to_string(),
        }
    }
}

pub trait QuerySubscriber: Send + Sync {
    fn on_update(&self, value: String);
    fn on_error(&self, message: String, value: Option<String>);
}

pub struct SubscriptionHandle {
    is_cancelled: AtomicBool,
    cancel_sender: Mutex<Option<Sender<()>>>,
    task_handle: Mutex<Option<JoinHandle<()>>>,
}

impl SubscriptionHandle {
    pub fn new(cancel_sender: Sender<()>, task_handle: JoinHandle<()>) -> Self {
        SubscriptionHandle {
            is_cancelled: AtomicBool::new(false),
            cancel_sender: Mutex::new(Some(cancel_sender)),
            task_handle: Mutex::new(Some(task_handle)),
        }
    }

    // Fixed: Return () instead of Result to match UniFFI interface
    pub fn cancel(&self) {
        if self.is_cancelled.swap(true, Ordering::AcqRel) {
            return; // Already cancelled
        }

        if let Some(sender) = self.cancel_sender.lock().take() {
            let _ = sender.send(()); // Silently ignore errors for UniFFI compatibility
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::Acquire)
    }
}

impl Drop for SubscriptionHandle {
    fn drop(&mut self) {
        self.cancel();
        if let Some(handle) = self.task_handle.lock().take() {
            handle.abort();
        }
    }
}

/// A wrapper around a [ConvexClient] and a [tokio::runtime::Runtime] used to
/// asynchronously call Convex functions, but exposed via synchronous entry points.
///
/// Designed specifically for Kotlin Multiplatform integration, providing a unified interface
/// that works across all KMP targets without requiring an external Tokio reactor.
pub struct MobileConvexClient {
    deployment_url: String,
    client_id: String,
    client: OnceCell<ConvexClient>,
    rt: tokio::runtime::Runtime,
    // Track active subscriptions to prevent leaks
    active_subscriptions: Arc<Mutex<HashSet<u64>>>,
    subscription_counter: AtomicU64,
    // Track if we're shutting down
    is_shutting_down: AtomicBool,
}

impl MobileConvexClient {
    /// Creates a new [MobileConvexClient].
    ///
    /// The internal [ConvexClient] doesn't get created/connected until the first public call.
    /// The `client_id` should be a string representing the name and version of the foreign client.
    pub fn new(deployment_url: String, client_id: String) -> Result<MobileConvexClient, ClientError> {
        Self::init_logging();

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("convex-mobile")
            .worker_threads(2) // Limit threads for mobile environments
            .build()
            .map_err(|e| ClientError::InternalError {
                msg: format!("Failed to create runtime: {}", e),
            })?;

        Ok(MobileConvexClient {
            deployment_url,
            client_id,
            client: OnceCell::new(),
            rt,
            active_subscriptions: Arc::new(Mutex::new(HashSet::new())),
            subscription_counter: AtomicU64::new(0),
            is_shutting_down: AtomicBool::new(false),
        })
    }

    fn init_logging() {
        #[cfg(all(debug_assertions, target_os = "android"))]
        {
            let _ = init_once(Config::default().with_max_level(LevelFilter::Debug));
        }
    }

    async fn connected_client(&self) -> Result<ConvexClient, ClientError> {
        let url = self.deployment_url.clone();

        self.client
            .get_or_try_init(async {
                ConvexClient::new(&url)
                    .await
                    .map_err(|e| ClientError::InternalError {
                        msg: format!("Failed to build client: {}", e),
                    })
            })
            .await
            .map(|client_ref| client_ref.clone()) // Cloning is required and correct for ConvexClient
            .map_err(|e| e.clone())
    }

    async fn internal_query(
        &self,
        name: String,
        args: HashMap<String, String>,
    ) -> Result<String, ClientError> {
        if self.is_shutting_down.load(Ordering::Acquire) {
            return Err(ClientError::InternalError {
                msg: "Client is shutting down".to_string(),
            });
        }

        let mut client = self.connected_client().await?;
        debug!("Executing query: {}", &name);

        let result = client
            .query(&name, parse_json_args(args)?)
            .await
            .map_err(|e| ClientError::InternalError {
                msg: format!("Query execution failed: {}", e),
            })?;

        debug!("Query completed: {}", &name);
        handle_direct_function_result(result)
    }

    async fn internal_subscribe(
        &self,
        name: String,
        args: HashMap<String, String>,
        subscriber: Box<dyn QuerySubscriber>,
    ) -> Result<Arc<SubscriptionHandle>, ClientError> {
        if self.is_shutting_down.load(Ordering::Acquire) {
            return Err(ClientError::InternalError {
                msg: "Client is shutting down".to_string(),
            });
        }

        let mut client = self.connected_client().await?;
        debug!("Creating subscription for: {}", name);

        // Fixed: Remove unnecessary mut
        let subscription = client
            .subscribe(&name, parse_json_args(args)?)
            .await
            .map_err(|e| ClientError::InternalError {
                msg: format!("Subscription creation failed: {}", e),
            })?;

        let (cancel_sender, cancel_receiver) = oneshot::channel::<()>();
        let subscriber: Arc<dyn QuerySubscriber> = Arc::from(subscriber);

        // Generate unique subscription ID and track it
        let subscription_id = self.subscription_counter.fetch_add(1, Ordering::Relaxed);
        self.active_subscriptions.lock().insert(subscription_id);

        let active_subscriptions = Arc::clone(&self.active_subscriptions);
        let subscription_name = name.clone();
        let is_shutting_down = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&is_shutting_down);

        let task_handle = self.rt.spawn(async move {
            let cancel_fut = cancel_receiver.fuse();
            pin_mut!(cancel_fut);
            pin_mut!(subscription);

            loop {
                select_biased! {
                    new_val = subscription.next().fuse() => {
                        // Check if we're shutting down
                        if shutdown_flag.load(Ordering::Acquire) {
                            break;
                        }

                        match new_val {
                            Some(result) => match result {
                                FunctionResult::Value(value) => {
                                    debug!(
                                        "Subscription update for {}: {:?}",
                                        &subscription_name, &value
                                    );
                                    // This double conversion is correct and necessary
                                    match serde_json::to_string(&serde_json::Value::from(value)) {
                                        Ok(json_value) => subscriber.on_update(json_value),
                                        Err(e) => {
                                            debug!(
                                                "Failed to serialize subscription value: {}",
                                                e
                                            );
                                            subscriber.on_error(
                                                "Serialization error".to_string(),
                                                Some(format!("Failed to serialize value: {}", e)),
                                            );
                                        }
                                    }
                                }
                                FunctionResult::ErrorMessage(msg) => {
                                    subscriber.on_error(msg, None);
                                }
                                FunctionResult::ConvexError(error) => {
                                    subscriber.on_error(
                                        "ConvexError".to_string(),
                                        Some(format!("{:?}", error)),
                                    );
                                }
                            },
                            None => {
                                debug!(
                                    "Subscription stream ended for: {}",
                                    &subscription_name
                                );
                                break;
                            }
                        }
                    },
                    _ = cancel_fut => {
                        debug!("Subscription cancelled for: {}", &subscription_name);
                        break;
                    },
                }
            }

            // Clean up subscription tracking
            active_subscriptions.lock().remove(&subscription_id);
            debug!("Subscription {} cleaned up", subscription_id);
        });

        Ok(Arc::new(SubscriptionHandle::new(cancel_sender, task_handle)))
    }

    async fn internal_mutation(
        &self,
        name: String,
        args: HashMap<String, String>,
    ) -> Result<FunctionResult, ClientError> {
        if self.is_shutting_down.load(Ordering::Acquire) {
            return Err(ClientError::InternalError {
                msg: "Client is shutting down".to_string(),
            });
        }

        let mut client = self.connected_client().await?;
        let parsed_args = parse_json_args(args)?;

        client
            .mutation(&name, parsed_args)
            .await
            .map_err(|e| ClientError::InternalError {
                msg: format!("Mutation execution failed: {}", e),
            })
    }

    async fn internal_action(
        &self,
        name: String,
        args: HashMap<String, String>,
    ) -> Result<FunctionResult, ClientError> {
        if self.is_shutting_down.load(Ordering::Acquire) {
            return Err(ClientError::InternalError {
                msg: "Client is shutting down".to_string(),
            });
        }

        let mut client = self.connected_client().await?;
        let parsed_args = parse_json_args(args)?;

        client
            .action(&name, parsed_args)
            .await
            .map_err(|e| ClientError::InternalError {
                msg: format!("Action execution failed: {}", e),
            })
    }

    async fn internal_set_auth(&self, token: Option<String>) -> Result<(), ClientError> {
        let mut client = self.connected_client().await?;
        client.set_auth(token).await;
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.client.get().is_some()
    }

    /// Synchronous wrapper for `query` exposed as `querySync`
    pub fn querySync(&self, name: String, args: HashMap<String, String>) -> Result<String, ClientError> {
        // Check if we're in an async context to prevent panics
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ClientError::InvalidContext {
                msg: "Cannot call querySync from within async context. Use queryAsync instead.".to_string(),
            });
        }

        self.rt.block_on(self.internal_query(name, args))
    }

    /// Synchronous wrapper for `subscribe` exposed as `subscribeSync`
    pub fn subscribeSync(
        &self,
        name: String,
        args: HashMap<String, String>,
        subscriber: Box<dyn QuerySubscriber>,
    ) -> Result<Arc<SubscriptionHandle>, ClientError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ClientError::InvalidContext {
                msg: "Cannot call subscribeSync from within async context. Use subscribeAsync instead.".to_string(),
            });
        }

        self.rt.block_on(self.internal_subscribe(name, args, subscriber))
    }

    /// Synchronous wrapper for `mutation` exposed as `mutationSync`
    pub fn mutationSync(&self, name: String, args: HashMap<String, String>) -> Result<String, ClientError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ClientError::InvalidContext {
                msg: "Cannot call mutationSync from within async context. Use mutationAsync instead.".to_string(),
            });
        }

        let fut = self.internal_mutation(name, args);
        let result = self.rt.block_on(fut)?;
        handle_direct_function_result(result)
    }

    /// Synchronous wrapper for `action` exposed as `actionSync`
    pub fn actionSync(&self, name: String, args: HashMap<String, String>) -> Result<String, ClientError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ClientError::InvalidContext {
                msg: "Cannot call actionSync from within async context. Use actionAsync instead.".to_string(),
            });
        }

        let fut = self.internal_action(name, args);
        let result = self.rt.block_on(fut)?;
        handle_direct_function_result(result)
    }

    /// Synchronous wrapper for `set_auth` exposed as `set_authSync`
    pub fn set_authSync(&self, token: Option<String>) -> Result<(), ClientError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ClientError::InvalidContext {
                msg: "Cannot call set_authSync from within async context. Use set_authAsync instead.".to_string(),
            });
        }

        self.rt.block_on(self.internal_set_auth(token))
    }

    // Fixed: Return () instead of Result to match UniFFI interface
    pub fn shutdown(&self) {
        debug!("Shutting down Convex client");

        // Mark as shutting down
        self.is_shutting_down.store(true, Ordering::Release);

        // Get count of active subscriptions
        let active_count = self.active_subscriptions.lock().len();
        if active_count > 0 {
            debug!("Waiting for {} active subscriptions to clean up", active_count);

            // Give subscriptions a moment to clean up gracefully
            std::thread::sleep(Duration::from_millis(100));
        }

        debug!("Convex client shutdown complete");
        // Note: We can't call shutdown_timeout because it moves the runtime
        // The runtime will be cleaned up when the client is dropped
    }

    pub fn active_subscription_count(&self) -> u64 {
        self.active_subscriptions.lock().len() as u64
    }

    pub fn is_shutting_down(&self) -> bool {
        self.is_shutting_down.load(Ordering::Acquire)
    }
}

fn parse_json_args(raw_args: HashMap<String, String>) -> Result<BTreeMap<String, Value>, ClientError> {
    raw_args
        .into_iter()
        .map(|(k, v)| {
            let json_value = serde_json::from_str::<serde_json::Value>(&v).map_err(|e| ClientError::InternalError {
                msg: format!("Invalid JSON for key '{}': {} – {}", k, v, e),
            })?;

            let convex_value = Value::try_from(json_value).map_err(|e| ClientError::InternalError {
                msg: format!("Invalid Convex value for key '{}': {}", k, e),
            })?;

            Ok((k, convex_value))
        })
        .collect()
}

fn handle_direct_function_result(result: FunctionResult) -> Result<String, ClientError> {
    match result {
        FunctionResult::Value(v) => {
            // This double conversion is correct and necessary for the mobile bridge
            serde_json::to_string(&serde_json::Value::from(v))
                .map_err(|e| ClientError::InternalError {
                    msg: format!("JSON serialization failed: {}", e),
                })
        }
        FunctionResult::ErrorMessage(msg) => Err(ClientError::ServerError { msg }),
        FunctionResult::ConvexError(error) => Err(ClientError::ConvexError {
            data: format!("{:?}", error),
        }),
    }
}

// UniFFI configuration for KMP. This binds the `querySync`, `subscribeSync`, etc. methods.
uniffi::include_scaffolding!("convexKmp");

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use convex::Value;
    use maplit::btreemap;
    use crate::parse_json_args;

    #[test]
    fn test_boolean_values_in_json_args() {
        let mut m = HashMap::new();
        m.insert(String::from("a"), String::from("false"));

        let result = parse_json_args(m).unwrap();
        assert_eq!(
            result.get(&String::from("a")),
            Some(&Value::Boolean(false))
        );
    }

    #[test]
    fn test_number_values_in_json_args() {
        let mut m = HashMap::new();
        m.insert(String::from("a"), String::from("42"));
        m.insert(String::from("b"), String::from("42.42"));

        let result = parse_json_args(m).unwrap();
        assert_eq!(result.get(&String::from("a")), Some(&Value::Float64(42.0)));
        assert_eq!(result.get(&String::from("b")), Some(&Value::Float64(42.42)));
    }

    #[test]
    fn test_list_values_in_json_args() {
        let mut m = HashMap::new();
        m.insert(String::from("a"), String::from("[1,2,3]"));
        m.insert(String::from("b"), String::from("[\"a\",\"b\",\"c\"]"));

        let result = parse_json_args(m).unwrap();
        assert_eq!(
            result.get(&String::from("a")),
            Some(&Value::Array(vec![
                Value::Float64(1.0),
                Value::Float64(2.0),
                Value::Float64(3.0)
            ]))
        );
        assert_eq!(
            result.get(&String::from("b")),
            Some(&Value::Array(vec![
                Value::String(String::from("a")),
                Value::String(String::from("b")),
                Value::String(String::from("c"))
            ]))
        );
    }

    #[test]
    fn test_object_values_in_json_args() {
        let mut m = HashMap::new();
        m.insert(String::from("a"), String::from("{\"a\":1,\"b\":\"foo\"}"));

        let result = parse_json_args(m).unwrap();
        assert_eq!(
            result.get(&String::from("a")),
            Some(&Value::Object(btreemap! {
                String::from("a") => Value::Float64(1.0),
                String::from("b") => Value::String(String::from("foo")),
            }))
        );
    }

    #[test]
    fn test_invalid_json_handling() {
        let mut m = HashMap::new();
        m.insert(String::from("invalid"), String::from("{invalid json}"));

        let result = parse_json_args(m);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_client_creation() {
        let client = MobileConvexClient::new(
            "https://test.convex.cloud".to_string(),
            "TestApp/1.0.0".to_string(),
        );
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(!client.is_initialized());
    }

    #[test]
    fn test_runtime_context_safety() {
        let client = MobileConvexClient::new(
            "https://test.convex.cloud".to_string(),
            "TestApp/1.0.0".to_string(),
        ).unwrap();

        // This should work fine - not in async context
        let result = client.querySync("test".to_string(), HashMap::new());
        // Will fail for connection reasons, but not context reasons
        assert!(matches!(result, Err(ClientError::InternalError { .. })));
    }
}