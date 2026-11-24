defmodule StruoSiteWeb.Docs.SidebarTest do
  use ExUnit.Case, async: true

  alias StruoSiteWeb.Docs.Sidebar
  alias StruoSiteWeb.Docs.Sidebar.Group
  alias StruoSiteWeb.Docs.Sidebar.Item

  test "picks the tab from the slug prefix" do
    assert Sidebar.tab_for_slug("/docs/reference") == :reference
    assert Sidebar.tab_for_slug("/docs/reference/cli/exec") == :reference
    assert Sidebar.tab_for_slug("/docs/reference-other") == :guides
    assert Sidebar.tab_for_slug("/docs/guide/why") == :guides
    assert Sidebar.tab_for_slug("/docs") == :guides
  end

  test "adjacent pages follow reading order without wrapping or linking to groups" do
    assert Sidebar.adjacent_pages("/docs/guide").previous == nil
    assert Sidebar.adjacent_pages("/docs/guide").next.slug == "/docs/guide/getting-started"
    assert Sidebar.adjacent_pages("/docs/guide/getting-started").previous.slug == "/docs/guide"
    assert Sidebar.adjacent_pages("/docs/reference/prelude/index").next == nil
    assert Sidebar.adjacent_pages("/docs/reference/events").next == nil
    assert Sidebar.adjacent_pages("/docs/reference").previous == nil
    assert Sidebar.adjacent_pages("/docs/unknown") == %{previous: nil, next: nil}
  end

  test "trees are groups of items with /docs slugs" do
    for %Group{items: items} <- Sidebar.guide_tree() ++ Sidebar.reference_tree() do
      assert Enum.all?(items, &valid_item?/1)
    end
  end

  test "a group's introduction is the group's own page, not an item" do
    groups = Sidebar.guide_tree() ++ Sidebar.reference_tree()
    typed_graph = Enum.find(groups, &(&1.label == "Typed Graph"))

    assert typed_graph.slug == "/docs/guide/graph"
    refute Enum.any?(typed_graph.items, &(&1.slug == "/docs/guide/graph"))
    assert Sidebar.adjacent_pages("/docs/guide/graph").next.slug == "/docs/guide/graph/ecosystems"
    assert Sidebar.adjacent_pages("/docs/guide/graph/ecosystems").previous.label == "Typed Graph"

    refute Enum.any?(groups, fn group -> Enum.any?(group.items, &(&1.label == "Overview")) end)
    assert Enum.find(groups, &(&1.label == "Toolchains")).slug == nil
  end

  test "item_active? matches the current slug" do
    item = %Item{label: "Why", slug: "/docs/guide/why"}
    assert Sidebar.item_active?(item, "/docs/guide/why")
    refute Sidebar.item_active?(item, "/docs/guide/other")
  end

  test "item_or_children_active? looks into nested items" do
    parent = %Item{
      label: "Apple",
      items: [%Item{label: "apple_library", slug: "/docs/reference/prelude/apple_library"}]
    }

    assert Sidebar.item_or_children_active?(parent, "/docs/reference/prelude/apple_library")
    refute Sidebar.item_or_children_active?(parent, "/docs/reference/prelude/other")
  end

  test "infrastructure providers are their own group: Tuist, then the rest by name" do
    infrastructure = Enum.find(Sidebar.guide_tree(), &(&1.label == "Infrastructure"))
    providers = Enum.find(Sidebar.guide_tree(), &(&1.label == "Infrastructure Providers"))

    assert Enum.map(providers.items, & &1.label) == [
             "Tuist",
             "Blaxel",
             "Boat",
             "Cloudflare",
             "Daytona",
             "Docker",
             "E2B",
             "HTTP Cache",
             "LangSmith",
             "Microsandbox",
             "Northflank",
             "Object Storage",
             "OpenSandbox",
             "Runloop",
             "Sprites",
             "Tensorlake",
             "Together",
             "Vercel Sandbox"
           ]

    for provider <- providers.items do
      assert File.exists?(
               "priv/docs" <> String.replace_prefix(provider.slug, "/docs", "") <> ".md"
             )
    end

    refute Enum.any?(
             infrastructure.items,
             &(&1.slug in Enum.map(providers.items, fn item -> item.slug end))
           )

    assert Enum.any?(infrastructure.items, &(&1.label == "Buildkite (Example)"))
  end

  test "natively supported toolchains live in their own group with logos" do
    toolchains = Enum.find(Sidebar.guide_tree(), &(&1.label == "Toolchains"))
    typed_graph = Enum.find(Sidebar.guide_tree(), &(&1.label == "Typed Graph"))

    labels = Enum.map(toolchains.items, & &1.label)

    assert labels == Enum.sort(labels)
    assert Enum.all?(toolchains.items, &is_binary(&1.icon))
    assert Enum.find(toolchains.items, &(&1.label == "Bazel")).icon == "bazel"
    refute Enum.any?(typed_graph.items, &(&1.label in labels))
  end

  test "exposes linting guides, commands, modules, and target kinds" do
    guide_slugs =
      Sidebar.guide_tree()
      |> Enum.flat_map(&flatten_items(&1.items))
      |> Enum.map(& &1.slug)

    reference_slugs =
      Sidebar.reference_tree()
      |> Enum.flat_map(&flatten_items(&1.items))
      |> Enum.map(& &1.slug)

    assert "/docs/guide/graph/linting" in guide_slugs
    assert "/docs/guide/local-execution/memory-limits" in guide_slugs
    assert "/docs/reference/cli/lint" in reference_slugs
    assert "/docs/reference/modules/linting" in reference_slugs
    assert "/docs/reference/prelude/ruff_lint" in reference_slugs
  end

  defp valid_item?(%Item{url: url}) when is_binary(url), do: true
  defp valid_item?(%Item{slug: slug, items: []}), do: String.starts_with?(slug, "/docs")
  defp valid_item?(%Item{items: items}), do: Enum.all?(items, &valid_item?/1)

  defp flatten_items(items) do
    Enum.flat_map(items, fn item -> [item | flatten_items(item.items)] end)
  end
end
