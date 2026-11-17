import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    self.contentViewController = flutterViewController
    self.minSize = NSSize(width: 900, height: 600)

    if let screenFrame = NSScreen.main?.visibleFrame {
      let w = min(1320.0, screenFrame.width * 0.88)
      let h = min(820.0, screenFrame.height * 0.86)
      let x = screenFrame.origin.x + (screenFrame.width - w) / 2.0
      let y = screenFrame.origin.y + (screenFrame.height - h) / 2.0
      self.setFrame(NSRect(x: x, y: y, width: w, height: h), display: true)
    } else {
      self.setFrame(NSRect(x: 120, y: 120, width: 1280, height: 800), display: true)
    }

    RegisterGeneratedPlugins(registry: flutterViewController)
    super.awakeFromNib()
  }
}
