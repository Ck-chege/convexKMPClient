package dev.convex.kmp.client


import java.nio.ByteBuffer
import java.nio.ByteOrder

actual fun Long.toByteArray(): ByteArray = ByteBuffer.allocate(java.lang.Long.BYTES).apply { order(
    ByteOrder.LITTLE_ENDIAN); putLong(this@toByteArray) }.array()
actual fun ByteArray.toLong(): Long = ByteBuffer.allocate(java.lang.Long.BYTES).apply { order(
    ByteOrder.LITTLE_ENDIAN); put(this@toLong); flip() }.getLong()
actual fun Double.toByteArray(): ByteArray = ByteBuffer.allocate(java.lang.Double.BYTES).apply { order(
    ByteOrder.LITTLE_ENDIAN); putDouble(this@toByteArray) }.array()
actual fun ByteArray.toDouble() : Double = ByteBuffer.allocate(java.lang.Double.BYTES).apply { order(
    ByteOrder.LITTLE_ENDIAN); put(this@toDouble); flip() }.getDouble()
