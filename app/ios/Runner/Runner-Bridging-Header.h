#import "GeneratedPluginRegistrant.h"

void orbit_ffi_free_string(char *ptr);
char *orbit_ffi_parse_pairing_uri(const char *uri_ptr);
char *orbit_ffi_noise_client_init(const char *pubkey_b64_ptr);
char *orbit_ffi_noise_client_finalize(unsigned long long handshake_handle, const char *msg2_b64_ptr);
char *orbit_ffi_noise_seal(unsigned long long channel_handle, const char *plain_b64_ptr);
char *orbit_ffi_noise_open(unsigned long long channel_handle, const char *record_b64_ptr);
void orbit_ffi_noise_close(unsigned long long channel_handle);
char *orbit_ffi_zstd_compress_wire(const char *json_ptr);
char *orbit_ffi_zstd_decompress_wire(const char *zstd_b64_ptr);
