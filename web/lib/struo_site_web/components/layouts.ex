defmodule StruoSiteWeb.Layouts do
  @moduledoc """
  This module holds layouts and related functionality
  used by your application.
  """
  use StruoSiteWeb, :html

  @icons_path Path.join([File.cwd!(), "deps", "noora", "lib", "noora", "icons"])
  @github_icon @icons_path |> Path.join("brand-github.svg") |> File.read!() |> String.trim()
  # Tabler's "brand-discord", which Noora's icon set does not include.
  @discord_icon """
  <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
    <path d="M8 12a1 1 0 1 0 2 0a1 1 0 0 0 -2 0" />
    <path d="M14 12a1 1 0 1 0 2 0a1 1 0 0 0 -2 0" />
    <path d="M15.5 17c0 1 1.5 3 2 3c1.5 0 2.833 -1.667 3.5 -3c.667 -1.667 .5 -5.833 -1.5 -11.5c-1.457 -1.015 -3 -1.34 -4.5 -1.5l-.972 1.923a11.913 11.913 0 0 0 -4.053 0l-.975 -1.923c-1.5 .16 -3.043 .485 -4.5 1.5c-2 5.667 -2.167 9.833 -1.5 11.5c.667 1.333 2 3 3.5 3c.5 0 2 -2 2 -3" />
    <path d="M7 16.5c3.5 1 6.5 1 10 0" />
  </svg>
  """

  @doc "The outline icon of a social link in the site header, as SVG markup."
  def social_icon(:github), do: @github_icon
  def social_icon(:discord), do: @discord_icon

  # Embed all files in layouts/* within this module.
  # The default root.html.heex file contains the HTML
  # skeleton of your application, namely HTML headers
  # and other static content.
  embed_templates "layouts/*"

  @doc "Default meta description used when a page does not set its own."
  def default_description do
    "Remote cache and execution, for the rest of us. Struo makes repository automation cacheable, " <>
      "remotely executable, and reusable across developers, coding agents, and machines."
  end

  @doc """
  Renders your app layout.

  This function is typically invoked from every template,
  and it often contains your application menu, sidebar,
  or similar.

  ## Examples

      <Layouts.app flash={@flash}>
        <h1>Content</h1>
      </Layouts.app>

  """
  attr :flash, :map, required: true, doc: "the map of flash messages"

  attr :current_scope, :map,
    default: nil,
    doc: "the current [scope](https://hexdocs.pm/phoenix/scopes.html)"

  slot :inner_block, required: true

  def app(assigns) do
    assigns =
      assigns
      |> assign(:github_icon, @github_icon)
      |> assign(:discord_icon, @discord_icon)

    ~H"""
    <div data-part="site-shell">
      <header data-part="site-header">
        <div data-part="site-bar">
          <a href={~p"/"} data-part="brand" aria-label="Struo home">
            <img
              data-part="brand-logo"
              src={~p"/images/struo-mark.webp"}
              width="213"
              height="208"
              alt=""
            />
            <span data-part="brand-name">Struo</span>
          </a>

          <nav data-part="site-nav" aria-label="Primary navigation">
            <a href="/docs">Docs</a>
            <a href={~p"/blog"}>Blog</a>
            <a href={~p"/changelog"}>Changelog</a>
          </nav>

          <div data-part="site-actions">
            <a
              data-part="site-social-link"
              href="https://github.com/tuist/struo"
              target="_blank"
              rel="noopener"
              aria-label="GitHub"
            >
              {Phoenix.HTML.raw(@github_icon)}
            </a>
            <a
              data-part="site-social-link"
              href="https://discord.gg/fTpB5e3rRp"
              target="_blank"
              rel="noopener"
              aria-label="Discord"
            >
              {Phoenix.HTML.raw(@discord_icon)}
            </a>
            <Noora.Button.ghost_button
              variant="secondary"
              size="large"
              data-part="mobile-menu-button"
              data-state="closed"
              aria-label="Toggle mobile menu"
              aria-expanded="false"
              aria-controls="mobile-menu"
            >
              <svg
                width="24"
                height="24"
                viewBox="0 0 24 24"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
                aria-hidden="true"
              >
                <path
                  data-part="bar-top"
                  d="M20 7.25C20.4142 7.25 20.75 7.58579 20.75 8C20.75 8.41421 20.4142 8.75 20 8.75H4C3.58579 8.75 3.25 8.41421 3.25 8C3.25 7.58579 3.58579 7.25 4 7.25H20Z"
                  fill="currentColor"
                />
                <path
                  data-part="bar-bottom"
                  d="M20 15.25C20.4142 15.25 20.75 15.5858 20.75 16C20.75 16.4142 20.4142 16.75 20 16.75H4C3.58579 16.75 3.25 16.4142 3.25 16C3.25 15.5858 3.58579 15.25 4 15.25H20Z"
                  fill="currentColor"
                />
              </svg>
            </Noora.Button.ghost_button>
          </div>
        </div>

        <nav data-part="mobile-menu" id="mobile-menu" aria-label="Mobile navigation">
          <a data-part="item" href="/docs">Docs</a>
          <a data-part="item" href={~p"/blog"}>Blog</a>
          <a data-part="item" href={~p"/changelog"}>Changelog</a>
          <a data-part="item" href="https://github.com/tuist/struo" target="_blank" rel="noopener">
            GitHub
          </a>
          <a data-part="item" href="https://discord.gg/fTpB5e3rRp" target="_blank" rel="noopener">
            Discord
          </a>
        </nav>
      </header>

      <main data-part="site-main">
        {render_slot(@inner_block)}
      </main>

      <footer data-part="site-footer">
        <div data-part="footer-brand">
          <img
            data-part="brand-logo"
            src={~p"/images/struo-mark.webp"}
            width="213"
            height="208"
            alt=""
          />
          <span>Struo</span>
        </div>
        <p>
          Remote cache &amp; execution, for the rest of us. <br /> Built with
          <span data-part="heart">♥</span>
          by <a href="https://tuist.dev" target="_blank" rel="noopener">Tuist</a>.
        </p>
        <nav data-part="footer-nav" aria-label="Footer navigation">
          <a href="/docs">Documentation</a>
          <a href={~p"/blog"}>Blog</a>
          <a href={~p"/changelog"}>Changelog</a>
          <a href="https://github.com/tuist/struo">GitHub</a>
          <a href="https://discord.gg/fTpB5e3rRp" target="_blank" rel="noopener">Discord</a>
        </nav>
      </footer>

      <.flash_group flash={@flash} />
    </div>
    """
  end

  @doc """
  Shows the flash group with standard titles and content.

  ## Examples

      <.flash_group flash={@flash} />
  """
  attr :flash, :map, required: true, doc: "the map of flash messages"
  attr :id, :string, default: "flash-group", doc: "the optional id of flash container"

  def flash_group(assigns) do
    ~H"""
    <div id={@id} data-part="flash-group" aria-live="polite">
      <p :if={message = Phoenix.Flash.get(@flash, :info)} data-part="flash-info">{message}</p>
      <p :if={message = Phoenix.Flash.get(@flash, :error)} data-part="flash-error">{message}</p>
    </div>
    """
  end
end
