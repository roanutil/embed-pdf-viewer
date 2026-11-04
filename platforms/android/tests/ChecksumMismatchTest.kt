package com.embedpdf.tests

import uniffi.epdf_ffi.uniffiEnsureInitialized

fun main() {
    val failure = runCatching { uniffiEnsureInitialized() }.exceptionOrNull()
    check(failure != null) { "A deliberately wrong expected checksum was accepted" }
    val messages = generateSequence(failure) { it.cause }.mapNotNull { it.message }.toList()
    check(messages.any { it.contains("UniFFI API checksum mismatch") }) {
        "Expected checksum rejection, received $messages"
    }
    println("ChecksumMismatchTest: native integrity gate rejected the deliberately wrong checksum")
}
