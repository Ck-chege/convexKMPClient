package dev.convex.kmp.client

actual fun Long.toByteArray(): ByteArray {
    val bytes = ByteArray(8)
    var value = this
    for (i in 7 downTo 0) {
        bytes[i] = (value and 0xFF).toByte()
        value = value shr 8
    }
    return bytes
}

actual fun ByteArray.toLong(): Long {
    var result = 0L
    for (i in 0..7) {
        result = (result shl 8) or (this[i].toUByte().toLong())
    }
    return result
}

actual fun Double.toByteArray(): ByteArray {
    val bits = this.toBits()
    return bits.toByteArray()
}

actual fun ByteArray.toDouble(): Double {
    val bits = this.toLong()
    return Double.fromBits(bits)
}
