import Foundation
import Product
import XCTest

final class ProductTests: XCTestCase {
    func testProduct() throws {
        let environment = ProcessInfo.processInfo.environment
        if environment["STRUO_FIXTURE_SKIP"] == "1" {
            throw XCTSkip("fixture skip")
        }
        XCTAssertEqual(covered(), 7)
        if environment["STRUO_FIXTURE_UNUSED"] == "1" {
            XCTAssertEqual(unused(), 13)
        }
        if environment["STRUO_FIXTURE_CHILD"] == "1" {
            let child = Process()
            child.executableURL = URL(fileURLWithPath: "/usr/bin/true")
            try child.run()
            child.waitUntilExit()
        }
        XCTAssertNotEqual(environment["STRUO_FIXTURE_FAIL"], "1")
    }
}
