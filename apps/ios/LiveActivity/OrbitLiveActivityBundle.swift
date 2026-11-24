import SwiftUI
import WidgetKit

@main
struct OrbitLiveActivityBundle: WidgetBundle {
    var body: some Widget {
        #if canImport(ActivityKit)
        if #available(iOS 16.2, *) {
            OrbitLiveActivityWidget()
        }
        #endif
    }
}
