defmodule StruoSite.ChangelogTest do
  use ExUnit.Case, async: true

  alias StruoSite.Changelog

  test "entry titles drop the quotes that wrap a frontmatter value" do
    titles = Enum.map(Changelog.entries(), & &1.title)

    assert "New blog post: Automation needs a git" in titles
    refute Enum.any?(titles, &String.starts_with?(&1, ["\"", "'"]))
  end
end
