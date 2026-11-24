defmodule AlphaTest do
  use ExUnit.Case
  test "same name" do
    if System.get_env("STRUO_FIXTURE_SLOW"), do: Process.sleep(5_000)
    if System.get_env("STRUO_FIXTURE_CHILD"), do: System.cmd("true", [])
    if System.get_env("STRUO_FIXTURE_FAIL"), do: flunk("fixture failure")
    assert Alpha.value() == 17
  end
end

defmodule BetaTest do
  use ExUnit.Case
  test "same name" do
    assert Beta.value() == 31
  end
end
