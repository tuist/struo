import Store
import Util
import SwiftUI

func readerConfiguration() -> String {
    #if TEST
    return "reader-test-configuration"
    #else
    return "reader-debug-configuration"
    #endif
}

@main
struct ReaderApp: App {
    var body: some Scene {
        WindowGroup { Text(readerConfiguration() + storeConfiguration() + utilConfiguration()) }
    }
}
