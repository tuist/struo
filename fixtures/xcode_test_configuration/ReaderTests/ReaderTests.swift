import Store
import XCTest
@testable import Reader

final class ReaderTests: XCTestCase {
    func testBuiltWithTheSchemeTestConfiguration() {
        XCTAssertEqual(readerConfiguration(), "reader-test-configuration")
        XCTAssertEqual(storeConfiguration(), "store-test-configuration")
    }
}
