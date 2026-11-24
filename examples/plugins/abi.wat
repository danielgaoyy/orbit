;; Scratch regions in pages: static data 0, request 1..5, host I/O 5..9,
;; response 9..13. A fresh instance processes exactly one request.
(memory (export "memory") 13 13)
(global (export "orbit_host_buffer") i32 (i32.const 327680))
(global $out (mut i32) (i32.const 589824))
(func (export "orbit_alloc") (param i32) (result i32) i32.const 65536)
(func $append (param $p i32) (param $len i32)
  (memory.copy (global.get $out) (local.get $p) (local.get $len))
  (global.set $out (i32.add (global.get $out) (local.get $len))))
(func $finish (result i64)
  (i64.or (i64.shl (i64.const 589824) (i64.const 32))
          (i64.extend_i32_u (i32.sub (global.get $out) (i32.const 589824)))))
;; The v1 envelope writes the "kind" discriminator first. Both variants are
;; daemon-constructed, never inferred from matching text in caller input.
(func $is_call (param $req i32) (result i32)
  (i32.eq (i32.load8_u offset=9 (local.get $req)) (i32.const 99)))
