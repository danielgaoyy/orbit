(func $nonempty (param $p i32) (param $n i32) (result i32) (local $r i64)
  (local.set $r (call $read (local.get $p) (local.get $n)))
  (i32.and (i64.ne (local.get $r) (i64.const -1))
           (i32.ne (i32.wrap_i64 (local.get $r)) (i32.const 0))))
(func (export "orbit_invoke") (param $req i32) (param i32) (result i64) (local $mask i32)
  call $path0 call $nonempty
  call $path1 call $nonempty i32.const 1 i32.shl i32.or
  call $path2 call $nonempty i32.const 2 i32.shl i32.or
  local.set $mask
  ;; guest.rs emits the matching operation result or UI card here.
