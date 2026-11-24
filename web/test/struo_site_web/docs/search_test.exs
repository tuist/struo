defmodule StruoSiteWeb.Docs.SearchTest do
  use ExUnit.Case, async: true

  alias StruoSiteWeb.Docs
  alias StruoSiteWeb.Docs.Search

  @page "/docs/guide/getting-started"

  setup_all do
    {:ok, page} = Docs.get_page(["guide", "getting-started"])
    %{page: page}
  end

  test "a page's title finds the page first, whatever the case", %{page: page} do
    assert [%{kind: :page, url: @page} = result | _] = Search.search(String.upcase(page.title))
    assert text(result.title) == page.title
    assert Enum.any?(result.title, fn {_text, highlighted?} -> highlighted? end)
  end

  test "a heading finds its section, linked to the heading's anchor", %{page: page} do
    heading = Enum.find(page.headings, &(&1.level == 3))

    results = Search.search(heading.text)

    assert %{trail: trail} =
             Enum.find(results, &(&1.kind == :section and &1.url == "#{@page}##{heading.id}"))

    assert String.ends_with?(trail, page.title)
  end

  test "words that only appear in the text find the section with an excerpt around them" do
    assert [%{kind: :content, snippet: snippet} | _] =
             results = Search.search("mebibytes reserve")

    assert Enum.all?(results, &(&1.kind == :content))
    assert {"mebibytes", true} in snippet
    assert text(snippet) =~ "250 mebibytes"
  end

  test "every word has to match, and a word may match as part of a longer one" do
    assert [%{kind: :page, url: @page} | _] = Search.search("gett start")
    assert Search.search("getting zzzqq") == []
  end

  test "an empty query has no results" do
    assert Search.search("") == []
    assert Search.search("   ") == []
  end

  test "results link to pages and headings that exist" do
    for query <- ["cache", "bazel", "apple_library", "remote execution"],
        result <- Search.search(query) do
      [path | anchor] = String.split(result.url, "#")
      "/docs/" <> rest = path

      assert {:ok, page} = Docs.get_page(String.split(rest, "/")), "no page at #{result.url}"

      for id <- anchor do
        assert id in Enum.map(page.headings, & &1.id), "no heading at #{result.url}"
      end
    end
  end

  test "suggestions are the guide's opening pages" do
    assert %{kind: :page, url: "/docs/guide/getting-started"} =
             Enum.find(Search.suggestions(), &(text(&1.title) == "Getting Started"))
  end

  defp text(segments), do: Enum.map_join(segments, fn {text, _highlighted?} -> text end)
end
