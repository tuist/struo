Describe 'product'
  Include ./product.sh
  Include ./lifecycle.sh
  Before 'before'
  After 'after'
  It 'returns a value'
    Skip if 'intentional skip' [ "${STRUO_FIXTURE_SKIP:-}" = 1 ]
    if [ "${STRUO_FIXTURE_CHILD:-}" = 1 ]; then bash -c ':'; fi
    When call value
    if [ "${STRUO_FIXTURE_FAIL:-}" = 1 ]; then
      The output should equal 42
    else
      The output should equal 41
    fi
  End
End
