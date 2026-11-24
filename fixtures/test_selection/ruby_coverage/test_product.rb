require 'minitest/autorun'
require_relative 'product'

class ProductTest < Minitest::Test
  def test_value
    assert_equal 41, Product.value
  end

  def test_double
    assert_equal 82, Product.value * 2
  end
end
