package example

class ProductTest {
    fun testValue() {
        check(productValue() == 41)
    }

    fun testTwice() {
        check(productValue() * 2 == 82)
    }
}
