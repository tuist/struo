public func storeConfiguration() -> String {
    #if TEST
    return "store-test-configuration"
    #else
    return "store-debug-configuration"
    #endif
}
