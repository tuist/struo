require_relative 'product'

RSpec.describe Product do
  it('returns its value') { expect(Product.value).to eq(41) }
end
