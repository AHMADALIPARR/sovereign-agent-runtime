// crypto.c — the native crypto backend. All primitives come from
// OpenSSL's EVP layer (a maintained library): SHA-2 digests,
// HMAC, AES-256-GCM (authenticated encryption), Ed25519
// signatures. Nothing here implements a cryptographic primitive
// from scratch; this file is argument handling and EVP calls.
//
// Protocol (argv in, one JSON line out):
//   cryptool hash    <algo> <data_hex>
//   cryptool hmac    <algo> <key_hex> <data_hex>
//   cryptool encrypt <key_hex> <plaintext_hex>     (AES-256-GCM)
//   cryptool decrypt <key_hex> <iv_hex> <ct_hex> <tag_hex>
//   cryptool sign    <seed_hex> <message_hex>      (Ed25519)
//   cryptool verify  <pubkey_hex> <message_hex> <sig_hex>
// Output: {"ok":true,"result":{...}} | {"ok":false,"error":"..."}

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <openssl/evp.h>
#include <openssl/rand.h>
#include <openssl/core_names.h>
#include <openssl/params.h>

static int hexval(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
static unsigned char *unhex(const char *s, size_t *out_len) {
    size_t n = strlen(s);
    if (n % 2) return NULL;
    unsigned char *buf = malloc(n / 2 + 1);
    for (size_t i = 0; i < n / 2; i++) {
        int hi = hexval(s[2 * i]), lo = hexval(s[2 * i + 1]);
        if (hi < 0 || lo < 0) { free(buf); return NULL; }
        buf[i] = (unsigned char)(hi * 16 + lo);
    }
    *out_len = n / 2;
    return buf;
}
static void print_hex(const unsigned char *buf, size_t len) {
    for (size_t i = 0; i < len; i++) printf("%02x", buf[i]);
}
static int fail(const char *msg) {
    printf("{\"ok\":false,\"error\":\"%s\"}\n", msg);
    return 0;
}

static int do_hash(const char *algo, const char *data_hex) {
    const EVP_MD *md = EVP_get_digestbyname(algo);
    if (!md) return fail("unknown digest algorithm");
    size_t dlen;
    unsigned char *data = unhex(data_hex, &dlen);
    if (!data) return fail("bad data hex");
    unsigned char digest[EVP_MAX_MD_SIZE];
    unsigned int outlen = 0;
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    EVP_DigestInit_ex(ctx, md, NULL);
    EVP_DigestUpdate(ctx, data, dlen);
    EVP_DigestFinal_ex(ctx, digest, &outlen);
    EVP_MD_CTX_free(ctx);
    printf("{\"ok\":true,\"result\":{\"algorithm\":\"%s\",\"digest_hex\":\"", algo);
    print_hex(digest, outlen);
    printf("\"}}\n");
    free(data);
    return 0;
}

static int do_hmac(const char *algo, const char *key_hex, const char *data_hex) {
    size_t klen, dlen;
    unsigned char *key = unhex(key_hex, &klen);
    unsigned char *data = unhex(data_hex, &dlen);
    if (!key || !data) return fail("bad hex input");
    EVP_MAC *mac = EVP_MAC_fetch(NULL, "HMAC", NULL);
    if (!mac) return fail("HMAC unavailable");
    EVP_MAC_CTX *ctx = EVP_MAC_CTX_new(mac);
    char digest_name[32];
    snprintf(digest_name, sizeof digest_name, "%s", algo);
    // OpenSSL wants the digest name upper-cased for HMAC params
    for (char *p = digest_name; *p; p++) if (*p >= 'a' && *p <= 'z') *p -= 32;
    OSSL_PARAM params[2];
    params[0] = OSSL_PARAM_construct_utf8_string(OSSL_MAC_PARAM_DIGEST, digest_name, 0);
    params[1] = OSSL_PARAM_construct_end();
    unsigned char out[EVP_MAX_MD_SIZE];
    size_t outlen = 0;
    EVP_MAC_init(ctx, key, klen, params);
    EVP_MAC_update(ctx, data, dlen);
    EVP_MAC_final(ctx, out, &outlen, sizeof out);
    EVP_MAC_CTX_free(ctx);
    EVP_MAC_free(mac);
    printf("{\"ok\":true,\"result\":{\"algorithm\":\"%s\",\"mac_hex\":\"", algo);
    print_hex(out, outlen);
    printf("\"}}\n");
    free(key); free(data);
    return 0;
}

static int do_encrypt(const char *key_hex, const char *pt_hex) {
    size_t klen, plen;
    unsigned char *key = unhex(key_hex, &klen);
    unsigned char *pt = unhex(pt_hex, &plen);
    if (!key || klen != 32) return fail("AES-256 key must be 32 bytes");
    if (!pt) return fail("bad plaintext hex");
    unsigned char iv[12];
    RAND_bytes(iv, sizeof iv);
    unsigned char *ct = malloc(plen + 16);
    unsigned char tag[16];
    EVP_CIPHER_CTX *ctx = EVP_CIPHER_CTX_new();
    int len = 0, ok = 1;
    EVP_EncryptInit_ex(ctx, EVP_aes_256_gcm(), NULL, NULL, NULL);
    EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, sizeof iv, NULL);
    EVP_EncryptInit_ex(ctx, NULL, NULL, key, iv);
    if (plen) ok &= EVP_EncryptUpdate(ctx, ct, &len, pt, (int)plen);
    int ctlen = len;
    ok &= EVP_EncryptFinal_ex(ctx, ct + ctlen, &len);
    ok &= EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_GET_TAG, sizeof tag, tag);
    EVP_CIPHER_CTX_free(ctx);
    if (!ok) return fail("encryption failed");
    printf("{\"ok\":true,\"result\":{\"iv_hex\":\"");
    print_hex(iv, sizeof iv);
    printf("\",\"ciphertext_hex\":\"");
    print_hex(ct, ctlen);
    printf("\",\"tag_hex\":\"");
    print_hex(tag, sizeof tag);
    printf("\"}}\n");
    free(key); free(pt); free(ct);
    return 0;
}

static int do_decrypt(const char *key_hex, const char *iv_hex, const char *ct_hex, const char *tag_hex) {
    size_t klen, ilen, clen, tlen;
    unsigned char *key = unhex(key_hex, &klen);
    unsigned char *iv = unhex(iv_hex, &ilen);
    unsigned char *ct = unhex(ct_hex, &clen);
    unsigned char *tag = unhex(tag_hex, &tlen);
    if (!key || klen != 32) return fail("AES-256 key must be 32 bytes");
    if (!iv || ilen != 12 || !ct || !tag || tlen != 16) return fail("bad iv/ciphertext/tag");
    unsigned char *pt = malloc(clen + 1);
    EVP_CIPHER_CTX *ctx = EVP_CIPHER_CTX_new();
    int len = 0;
    EVP_DecryptInit_ex(ctx, EVP_aes_256_gcm(), NULL, NULL, NULL);
    EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, (int)ilen, NULL);
    EVP_DecryptInit_ex(ctx, NULL, NULL, key, iv);
    EVP_DecryptUpdate(ctx, pt, &len, ct, (int)clen);
    int ptlen = len;
    EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_TAG, (int)tlen, tag);
    int ok = EVP_DecryptFinal_ex(ctx, pt + ptlen, &len);
    EVP_CIPHER_CTX_free(ctx);
    if (ok <= 0) { free(pt); return fail("GCM tag verification failed — ciphertext refused"); }
    printf("{\"ok\":true,\"result\":{\"plaintext_hex\":\"");
    print_hex(pt, ptlen);
    printf("\"}}\n");
    free(key); free(iv); free(ct); free(tag); free(pt);
    return 0;
}

static int do_sign(const char *seed_hex, const char *msg_hex) {
    size_t slen, mlen;
    unsigned char *seed = unhex(seed_hex, &slen);
    unsigned char *msg = unhex(msg_hex, &mlen);
    if (!seed || slen != 32) return fail("Ed25519 seed must be 32 bytes");
    if (!msg) return fail("bad message hex");
    EVP_PKEY *pkey = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32);
    if (!pkey) return fail("key construction failed");
    unsigned char pub[32];
    size_t publen = sizeof pub;
    EVP_PKEY_get_raw_public_key(pkey, pub, &publen);
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    unsigned char sig[64];
    size_t siglen = sizeof sig;
    EVP_DigestSignInit(ctx, NULL, NULL, NULL, pkey);
    EVP_DigestSign(ctx, sig, &siglen, msg, mlen);
    EVP_MD_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    printf("{\"ok\":true,\"result\":{\"public_key_hex\":\"");
    print_hex(pub, publen);
    printf("\",\"signature_hex\":\"");
    print_hex(sig, siglen);
    printf("\"}}\n");
    free(seed); free(msg);
    return 0;
}

static int do_verify(const char *pub_hex, const char *msg_hex, const char *sig_hex) {
    size_t plen, mlen, slen;
    unsigned char *pub = unhex(pub_hex, &plen);
    unsigned char *msg = unhex(msg_hex, &mlen);
    unsigned char *sig = unhex(sig_hex, &slen);
    if (!pub || plen != 32 || !msg || !sig || slen != 64) return fail("bad verify inputs");
    EVP_PKEY *pkey = EVP_PKEY_new_raw_public_key(EVP_PKEY_ED25519, NULL, pub, 32);
    if (!pkey) return fail("key construction failed");
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    EVP_DigestVerifyInit(ctx, NULL, NULL, NULL, pkey);
    int ok = EVP_DigestVerify(ctx, sig, slen, msg, mlen);
    EVP_MD_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    printf("{\"ok\":true,\"result\":{\"valid\":%s}}\n", ok == 1 ? "true" : "false");
    free(pub); free(msg); free(sig);
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 2) return fail("no operation");
    const char *op = argv[1];
    if (!strcmp(op, "hash") && argc == 4) return do_hash(argv[2], argv[3]);
    if (!strcmp(op, "hmac") && argc == 5) return do_hmac(argv[2], argv[3], argv[4]);
    if (!strcmp(op, "encrypt") && argc == 4) return do_encrypt(argv[2], argv[3]);
    if (!strcmp(op, "decrypt") && argc == 6) return do_decrypt(argv[2], argv[3], argv[4], argv[5]);
    if (!strcmp(op, "sign") && argc == 4) return do_sign(argv[2], argv[3]);
    if (!strcmp(op, "verify") && argc == 5) return do_verify(argv[2], argv[3], argv[4]);
    return fail("unknown operation or wrong argument count");
}
