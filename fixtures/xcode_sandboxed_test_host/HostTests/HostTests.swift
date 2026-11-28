import XCTest
@testable import Host_App

final class HostTests: XCTestCase {
    func testGreeting() {
        XCTAssertEqual(greeting(), "hosted")
    }
}
