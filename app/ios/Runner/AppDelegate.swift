import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  @objc public static func retainOrbitRustFfiSymbols() -> Int {
    let funcs: [UnsafeRawPointer] = [
      unsafeBitCast(orbit_ffi_free_string as @convention(c) (UnsafeMutablePointer<CChar>?) -> Void, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_parse_pairing_uri as @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_noise_client_init as @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_noise_client_finalize as @convention(c) (UInt64, UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_noise_seal as @convention(c) (UInt64, UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_noise_open as @convention(c) (UInt64, UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_noise_close as @convention(c) (UInt64) -> Void, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_zstd_compress_wire as @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
      unsafeBitCast(orbit_ffi_zstd_decompress_wire as @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, to: UnsafeRawPointer.self),
    ]
    return funcs.count
  }

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    _ = AppDelegate.retainOrbitRustFfiSymbols()
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
  }
}
