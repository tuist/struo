import XCTest
@testable import Host

final class HostTests: XCTestCase {
    func testGreeting() {
        XCTAssertEqual(greeting(), "hosted")
    }
}
