test('returns the product value', () => {
  expect(require('./product.cjs').value()).toBe(41)
})
