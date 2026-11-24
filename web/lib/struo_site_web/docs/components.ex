defmodule StruoSiteWeb.Docs.Components do
  @moduledoc """
  Layout, sidebar, and table-of-contents components for the documentation.
  Navigation is plain lists of links; buttons, dropdowns, and icons come from
  Noora.
  """
  use Phoenix.Component
  use Noora

  alias Phoenix.LiveView.JS
  alias StruoSiteWeb.Docs.Sidebar
  alias StruoSiteWeb.Layouts

  attr :current_slug, :string, required: true
  attr :tab, :atom, required: true
  attr :headings, :list, required: true
  attr :markdown, :string, required: true
  slot :inner_block, required: true

  def layout(assigns) do
    assigns = assign(assigns, :tree, Sidebar.tree_for_tab(assigns.tab))
    page_layout(assigns)
  end

  embed_templates "components/*"

  # Sidebar icons that Noora's icon set has. The rest are drawn by the
  # stylesheet from the site's own files.
  @noora_icons %{
    "android" => "brand_android",
    "apple" => "brand_apple",
    "bazel" => "brand_bazel",
    "containers" => "stack_2",
    "elixir" => "brand_elixir",
    "rust" => "brand_rust",
    "swift" => "brand_swift"
  }

  attr :item, :map, required: true
  attr :current_slug, :string, required: true

  defp nav_link(assigns) do
    assigns = assign(assigns, :noora_icon, @noora_icons[assigns.item.icon])

    ~H"""
    <.link
      patch={docs_path(@item.slug)}
      data-part="nav-link"
      data-selected={Sidebar.item_active?(@item, @current_slug)}
      phx-click={close_mobile_navigation()}
    >
      <span :if={@noora_icon} data-part="nav-item-icon" aria-hidden="true">
        <.icon name={@noora_icon} />
      </span>
      <span
        :if={@item.icon && !@noora_icon}
        data-part="nav-item-icon"
        data-icon={@item.icon}
        aria-hidden="true"
      ></span>
      <span data-part="label">{@item.label}</span>
    </.link>
    """
  end

  attr :id, :string, required: true
  attr :current_slug, :string, required: true
  attr :placement, :string, default: "bottom-start"

  # The label copies the page; the chevron opens the other ways to take it.
  defp copy_page_menu(assigns) do
    ~H"""
    <.menu
      id={@id}
      label="Copy page"
      trigger="split"
      on_action={JS.dispatch("docs:copy-page")}
      placement={@placement}
    >
      <.menu_item
        value="copy-markdown"
        label="Copy page"
        description="Copy page as markdown for LLMs"
        on_click={JS.dispatch("docs:copy-page")}
      >
        <:icon><.copy /></:icon>
      </.menu_item>
      <.menu_item
        value="view-markdown"
        label="View as Markdown"
        description="View this page as plain text"
        href={docs_markdown_path(@current_slug)}
      >
        <:icon><.icon name="markdown" /></:icon>
        <:extra><.external_link /></:extra>
      </.menu_item>
    </.menu>
    """
  end

  defp nav_indicator(assigns) do
    ~H"""
    <span data-part="indicator">
      <span data-part="indicator-down"><.chevron_down /></span>
      <span data-part="indicator-up"><.chevron_up /></span>
    </span>
    """
  end

  defp close_mobile_navigation do
    JS.remove_attribute("data-sidebar-open", to: "body")
    |> JS.remove_attribute("data-mobile-open", to: "#docs-sidebar")
    |> JS.set_attribute({"aria-expanded", "false"}, to: "#docs-mobile-menu-trigger")
    |> close_mobile_toc()
  end

  defp close_mobile_toc(js \\ %JS{}) do
    js
    |> JS.set_attribute({"data-state", "closed"}, to: "#docs-mobile-toc")
    |> JS.set_attribute({"aria-expanded", "false"}, to: "#docs-mobile-toc-trigger")
  end

  defp docs_path(slug), do: slug

  defp docs_markdown_path("/docs/" <> rest), do: "/docs-markdown/" <> rest
  defp docs_markdown_path(_), do: "/docs-markdown"

  defp slugify(label) do
    label
    |> String.downcase()
    |> String.replace(~r/[^a-z0-9]+/, "-")
    |> String.trim("-")
  end
end
