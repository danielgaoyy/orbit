;; Scan a JSON token while respecting nested containers and escaped quotes.
;; Titles remain JSON string literals throughout, so text cannot change the UI tree.
(func $ws (param $p i32) (result i32)
  (loop $more
    (if (i32.le_u (i32.load8_u (local.get $p)) (i32.const 32))
        (then (local.set $p (i32.add (local.get $p) (i32.const 1))) (br $more))))
  local.get $p)
(func $after_string (param $p i32) (result i32) (local $c i32)
  (loop $more
    (local.set $p (i32.add (local.get $p) (i32.const 1)))
    (local.set $c (i32.load8_u (local.get $p)))
    (if (i32.eq (local.get $c) (i32.const 34))
        (then (return (i32.add (local.get $p) (i32.const 1)))))
    (if (i32.eq (local.get $c) (i32.const 92))
        (then (local.set $p (i32.add (local.get $p) (i32.const 1)))))
    br $more)
  unreachable)
(func $after_value (param $p i32) (result i32) (local $depth i32) (local $c i32)
  (loop $more
    (local.set $c (i32.load8_u (local.get $p)))
    (if (i32.eq (local.get $c) (i32.const 34))
      (then (local.set $p (call $after_string (local.get $p)))
            (if (i32.eqz (local.get $depth)) (then (return (local.get $p)))) (br $more)))
    (if (i32.or (i32.eq (local.get $c) (i32.const 123)) (i32.eq (local.get $c) (i32.const 91)))
        (then (local.set $depth (i32.add (local.get $depth) (i32.const 1)))))
    (if (i32.or (i32.eq (local.get $c) (i32.const 125)) (i32.eq (local.get $c) (i32.const 93)))
        (then (if (i32.eqz (local.get $depth)) (then (return (local.get $p))))
              (local.set $depth (i32.sub (local.get $depth) (i32.const 1)))
              (if (i32.eqz (local.get $depth)) (then (return (i32.add (local.get $p) (i32.const 1)))))))
    (if (i32.and (i32.eqz (local.get $depth))
                 (i32.or (i32.eq (local.get $c) (i32.const 44)) (i32.le_u (local.get $c) (i32.const 32))))
        (then (return (local.get $p))))
    (local.set $p (i32.add (local.get $p) (i32.const 1))) br $more)
  unreachable)
(func $equal (param $a i32) (param $b i32) (param $n i32) (result i32)
  (loop $more
    (if (i32.eqz (local.get $n)) (then (return (i32.const 1))))
    (local.set $n (i32.sub (local.get $n) (i32.const 1)))
    (if (i32.ne (i32.load8_u (i32.add (local.get $a) (local.get $n)))
                (i32.load8_u (i32.add (local.get $b) (local.get $n))))
        (then (return (i32.const 0))))
    br $more) unreachable)
(func $member (param $p i32) (param $key i32) (param $len i32) (result i32)
  (local $end i32) (local $val i32)
  (loop $more
    (local.set $p (call $ws (i32.add (local.get $p) (i32.const 1))))
    (if (i32.ne (i32.load8_u (local.get $p)) (i32.const 34)) (then (return (i32.const 0))))
    (local.set $end (call $after_string (local.get $p)))
    (local.set $val (call $ws (i32.add (call $ws (local.get $end)) (i32.const 1))))
    (if (i32.and (i32.eq (i32.sub (local.get $end) (local.get $p)) (local.get $len))
                 (call $equal (local.get $p) (local.get $key) (local.get $len)))
        (then (return (local.get $val))))
    (local.set $p (call $ws (call $after_value (local.get $val))))
    (br_if $more (i32.eq (i32.load8_u (local.get $p)) (i32.const 44))))
  i32.const 0)
(func (export "orbit_invoke") (param $req i32) (param i32) (result i64)
  (local $data i64) (local $item i32) (local $title i32) (local $action i32)
  (if (call $is_call (local.get $req)) (then call $url call $get return))
  call $ui_begin call $append
  ;; First paint can be requested offline. Clicking Refresh loads live data.
  local.get $req call $action_id call $member local.tee $action
  i32.load8_u i32.const 34 i32.eq
  (if (then
    local.get $action call $refresh call $equal i32.eqz (if (then unreachable))
    call $url call $get local.set $data
    (local.set $item (call $ws (i32.wrap_i64 (i64.shr_u (local.get $data) (i64.const 32)))))
    (if (i32.ne (i32.load8_u (local.get $item)) (i32.const 91)) (then unreachable))
    (loop $rows
      (local.set $item (call $ws (i32.add (local.get $item) (i32.const 1))))
      (if (i32.eq (i32.load8_u (local.get $item)) (i32.const 123)) (then
        local.get $item call $title call $member local.tee $title
        i32.load8_u i32.const 34 i32.ne (if (then unreachable))
        local.get $item call $pull_request call $member
        (if (then call $pr call $append) (else call $issue call $append))
        local.get $title (i32.sub (call $after_string (local.get $title)) (local.get $title)) call $append
        call $end_row call $append
        (local.set $item (call $ws (call $after_value (local.get $item))))
        (if (i32.eq (i32.load8_u (local.get $item)) (i32.const 44))
            (then call $next call $append br $rows)))))))
  call $ui_end call $append call $finish)
