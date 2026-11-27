import SwiftUI

func greeting() -> String { "hosted" }

@main
struct HostApp: App {
    var body: some Scene {
        WindowGroup { Text(greeting()) }
    }
}
