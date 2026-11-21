package app.devti.android.core

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.devti_ffi.DevtiException
import uniffi.devti_ffi.PlatformSecretStore

/**
 * Secrets for Devti Core (the device's private key), sealed with an AES-256-GCM key
 * that never leaves the Android Keystore. Only the ciphertext is stored, in private
 * app storage that is excluded from backups.
 */
class KeystoreSecretStore(context: Context) : PlatformSecretStore {
    private val prefs = context.getSharedPreferences("devti_sealed_secrets", Context.MODE_PRIVATE)

    private fun masterKey(): SecretKey {
        val ks = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    override fun get(key: String): ByteArray? = guard {
        val stored = prefs.getString(key, null) ?: return@guard null
        val raw = Base64.decode(stored, Base64.NO_WRAP)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, masterKey(), GCMParameterSpec(128, raw.copyOfRange(0, IV_LEN)))
        cipher.doFinal(raw, IV_LEN, raw.size - IV_LEN)
    }

    override fun set(key: String, value: ByteArray) = guard {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, masterKey())
        val sealed = cipher.iv + cipher.doFinal(value)
        check(prefs.edit().putString(key, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()) {
            "could not save the secret"
        }
    }

    override fun delete(key: String) = guard {
        prefs.edit().remove(key).commit()
        Unit
    }

    override fun backendName(): String = "Android Keystore"

    private inline fun <T> guard(block: () -> T): T =
        try {
            block()
        } catch (e: DevtiException) {
            throw e
        } catch (e: Exception) {
            throw DevtiException.Failed("Android Keystore: ${e.message ?: e.javaClass.simpleName}")
        }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "devti.secret-wrapping.v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_LEN = 12
    }
}
