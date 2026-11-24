import Foundation
import Product
import Testing

@Test(.disabled(if: ProcessInfo.processInfo.environment["STRUO_FIXTURE_SKIP"] == "1"))
func exercisesProduct() throws {
    let environment = ProcessInfo.processInfo.environment
    #expect(covered() == 7)
    if environment["STRUO_FIXTURE_UNUSED"] == "1" {
        #expect(unused() == 13)
    }
    if environment["STRUO_FIXTURE_CHILD"] == "1" {
        let child = Process()
        child.executableURL = URL(fileURLWithPath: "/usr/bin/true")
        try child.run()
        child.waitUntilExit()
    }
    #expect(environment["STRUO_FIXTURE_FAIL"] != "1")
}
