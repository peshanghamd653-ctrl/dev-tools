;; A plugin that never returns. The host must stop it anyway.
;; Proves fuel metering halts a runaway guest instead of hanging DevOS.
(module
  (func (export "spin")
    (loop $forever
      br $forever)))
