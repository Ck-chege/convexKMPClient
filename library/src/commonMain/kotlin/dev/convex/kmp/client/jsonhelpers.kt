
@file:OptIn(ExperimentalEncodingApi::class)
package dev.convex.kmp.client


import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.JsonTransformingSerializer
import kotlin.io.encoding.Base64
import kotlin.io.encoding.ExperimentalEncodingApi

@PublishedApi
internal fun Map<*, *>.toJsonElement(): JsonElement {
    val map: MutableMap<String, JsonElement> = mutableMapOf()
    this.forEach {
        val key = it.key as? String ?: return@forEach
        val value = it.value ?: return@forEach
        when (value) {
            is Map<*, *> -> map[key] = (value).toJsonElement()
            is List<*> -> map[key] = value.toJsonElement()
            else -> map[key] = value.toJsonElement()
        }
    }
    return JsonObject(map)
}

@PublishedApi
internal fun List<*>.toJsonElement(): JsonElement {
    val list: MutableList<JsonElement> = mutableListOf()
    this.forEach {
        val value = it ?: return@forEach
        when (value) {
            is Map<*, *> -> list.add((value).toJsonElement())
            is List<*> -> list.add(value.toJsonElement())
            else -> list.add(value.toJsonElement())
        }
    }
    return JsonArray(list)
}

@PublishedApi
internal fun Any?.toJsonElement(): JsonElement {
    return when (this) {
        is Map<*, *> -> this.toJsonElement()
        is List<*> -> this.toJsonElement()
        is String -> JsonPrimitive(this)
        is Boolean -> JsonPrimitive(this)
        is Long  -> mapOf("\$integer" to Base64.encode(this.toByteArray())).toJsonElement()
        is Int -> mapOf("\$integer" to Base64.encode(this.toLong().toByteArray())).toJsonElement()
        is ByteArray -> mapOf("\$bytes" to Base64.encode(this)).toJsonElement()
        is Double -> when (this) {
            Double.NaN, Double.NEGATIVE_INFINITY, Double.POSITIVE_INFINITY -> mapOf("\$float" to Base64.encode(this.toByteArray())).toJsonElement()
            else -> JsonPrimitive(this)
        }
        is Float -> when (this) {
            Float.NaN, Float.NEGATIVE_INFINITY, Float.POSITIVE_INFINITY -> mapOf("\$float" to Base64.encode(this.toDouble().toByteArray())).toJsonElement()
            else -> JsonPrimitive(this)
        }
        is Number -> JsonPrimitive(this)
        null -> JsonNull
        else -> throw IllegalArgumentException("only maps, lists and JSON primitives supported; got $this")
    }
}

//expect fun Long.toByteArray(): ByteArray
//expect fun ByteArray.toLong(): Long
//expect fun Double.toByteArray(): ByteArray
//expect fun ByteArray.toDouble(): Double



//package dev.convex.kmp.client

fun Long.toByteArray(): ByteArray {
    val bytes = ByteArray(8)
    var value = this
    for (i in 7 downTo 0) {
        bytes[i] = (value and 0xFF).toByte()
        value = value shr 8
    }
    return bytes
}

fun ByteArray.toLong(): Long {
    var result = 0L
    for (i in 0..7) {
        result = (result shl 8) or (this[i].toUByte().toLong())
    }
    return result
}

fun Double.toByteArray(): ByteArray {
    val bits = this.toBits()
    return bits.toByteArray()
}

fun ByteArray.toDouble(): Double {
    val bits = this.toLong()
    return Double.fromBits(bits)
}



object Int64ToLongDecoder : JsonTransformingSerializer<Long>(Long.serializer()) {
    override fun transformDeserialize(element: JsonElement): JsonElement {
        if (element is JsonObject) {
            val v = element["\$integer"]!! as JsonPrimitive
            return JsonPrimitive(Base64.decode(v.content).toLong())
        }
        return element
    }
}


object Int64ToIntDecoder : JsonTransformingSerializer<Int>(Int.serializer()) {
    override fun transformDeserialize(element: JsonElement): JsonElement {
        if (element is JsonObject) {
            val v = element["\$integer"]!! as JsonPrimitive
            return JsonPrimitive(Base64.decode(v.content).toLong().toInt())
        }
        return element
    }
}

object Float64ToDoubleDecoder : JsonTransformingSerializer<Double>(Double.serializer()) {
    override fun transformDeserialize(element: JsonElement): JsonElement {
        if (element is JsonObject) {
            val v = element["\$float"]!! as JsonPrimitive
            return JsonPrimitive(Base64.decode(v.content).toDouble())
        }
        return element
    }
}

object Float64ToFloatDecoder : JsonTransformingSerializer<Float>(Float.serializer()) {
    override fun transformDeserialize(element: JsonElement): JsonElement {
        if (element is JsonObject) {
            val v = element["\$float"]!! as JsonPrimitive
            return JsonPrimitive(Base64.decode(v.content).toDouble().toFloat())
        }
        return element
    }
}