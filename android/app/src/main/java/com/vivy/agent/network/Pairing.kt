package com.vivy.agent.network

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import okhttp3.OkHttpClient
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

/**
 * Pairing with one PC. The connection is TLS with a certificate the PC made for
 * itself. The first time, the user types the code shown on the PC; both sides
 * prove they know it, bound to the certificate this phone actually sees, so
 * nobody else on the network can pose as the PC. After that, the PC's
 * certificate fingerprint is pinned and a secret token proves this phone.
 * Must match desktop/src-tauri/src/phone/pairing.rs.
 */
object Pairing {

    data class Paired(val fingerprint: String, val token: String, val pcName: String)

    private const val PREFS = "vivy_pairing"
    private val _paired = MutableStateFlow<Paired?>(null)
    val paired: StateFlow<Paired?> = _paired
    @Volatile private var loaded = false

    fun load(context: Context): Paired? {
        if (!loaded) {
            val p = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val fp = p.getString("fingerprint", null)
            val token = p.getString("token", null)
            _paired.value = if (fp != null && token != null) Paired(fp, token, p.getString("pc_name", "PC") ?: "PC") else null
            loaded = true
        }
        return _paired.value
    }

    fun save(context: Context, paired: Paired) {
        context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString("fingerprint", paired.fingerprint)
            .putString("token", paired.token)
            .putString("pc_name", paired.pcName)
            .apply()
        _paired.value = paired
        loaded = true
    }

    fun forget(context: Context) {
        context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().apply()
        _paired.value = null
        loaded = true
    }

    /** Uppercase, drop separators, and fix look-alike characters typed by mistake. */
    fun normalizeCode(input: String): String = input
        .filter { it.isLetterOrDigit() && it.code < 128 }
        .map {
            when (val c = it.uppercaseChar()) {
                'O' -> '0'
                'I', 'L' -> '1'
                else -> c
            }
        }
        .joinToString("")

    /** Codes are 12 characters, shown on the PC as XXXX-XXXX-XXXX. */
    fun isCompleteCode(input: String): Boolean = normalizeCode(input).length == 12

    fun proof(code: String, role: String, fingerprint: String, phoneNonce: String, pcNonce: String): String {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(normalizeCode(code).toByteArray(Charsets.US_ASCII), "HmacSHA256"))
        return hex(mac.doFinal("vivy-pair|$role|$fingerprint|$phoneNonce|$pcNonce".toByteArray(Charsets.UTF_8)))
    }

    fun nonce(): String = ByteArray(16).also { SecureRandom().nextBytes(it) }.let(::hex)

    /** Compare without leaking how much matched. */
    fun sameSecret(a: String, b: String): Boolean = MessageDigest.isEqual(a.toByteArray(), b.toByteArray())

    fun sha256Hex(bytes: ByteArray): String = hex(MessageDigest.getInstance("SHA-256").digest(bytes))

    private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    /**
     * Accepts only the pinned PC certificate. While pairing (nothing pinned
     * yet) it accepts the certificate but records its fingerprint, which the
     * pairing proof then checks.
     */
    class PinnedTrust(private val pinned: String?) : X509TrustManager {
        @Volatile var seenFingerprint: String? = null
            private set

        override fun checkServerTrusted(chain: Array<out X509Certificate>, authType: String?) {
            val leaf = chain.firstOrNull() ?: throw CertificateException("No certificate")
            val fp = sha256Hex(leaf.encoded)
            seenFingerprint = fp
            if (pinned != null && !sameSecret(fp, pinned)) {
                throw CertificateException("This isn't the PC this phone was paired with")
            }
        }

        override fun checkClientTrusted(chain: Array<out X509Certificate>, authType: String?) {
            throw CertificateException("Not used")
        }

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    /**
     * A client that trusts exactly the PC's own certificate. Host names aren't
     * checked: the PC is reached by IP, and its identity is the pinned
     * certificate (or the pairing proof), not a name.
     */
    fun client(base: OkHttpClient, trust: PinnedTrust): OkHttpClient {
        val ssl = SSLContext.getInstance("TLS").apply { init(null, arrayOf(trust), SecureRandom()) }
        return base.newBuilder()
            .sslSocketFactory(ssl.socketFactory, trust)
            .hostnameVerifier { _, _ -> true }
            .build()
    }
}
