defmodule Mix.Tasks.Marketing.Gen.OgImages do
  @shortdoc "Generate marketing Open Graph images"

  @moduledoc """
  Generates Open Graph images for the marketing pages, changelog, and blog.

  Cards are rendered to `priv/static/images/og/<page>.jpg` with headless Chrome.
  Like the docs task, it degrades gracefully when Chrome is unavailable.
  """
  use Mix.Task

  alias StruoSite.Blog
  alias StruoSite.Changelog
  alias StruoSiteWeb.ChangelogHTML
  alias StruoSiteWeb.Docs.OgImage
  alias StruoSiteWeb.Docs.OgImageRenderer

  @output "priv/static/images/og"
  @fonts_dir "priv/static/fonts"
  @logo "priv/static/images/struo-mark.webp"

  @static_pages [
    %{
      file: "home.jpg",
      title: "Remote cache & execution, for the rest of us",
      description:
        "Cacheable, remotely executable, reusable repository automation across every language."
    },
    %{
      file: "changelog.jpg",
      title: "Changelog",
      subtitle: "Changelog"
    },
    %{
      file: "blog.jpg",
      title: "Ideas for building Struo",
      subtitle: "Blog"
    }
  ]

  @impl true
  def run(_args) do
    {:ok, _} = Application.ensure_all_started(:briefly)

    case OgImageRenderer.start() do
      {:ok, renderer} ->
        try do
          File.mkdir_p!(@output)
          pages = pages()

          for page <- pages do
            html =
              OgImage.render_html(
                title: page.title,
                description: Map.get(page, :description),
                category: Map.get(page, :category),
                subtitle: Map.get(page, :subtitle),
                fonts_dir: @fonts_dir,
                logo_path: @logo
              )

            case OgImageRenderer.render(renderer, html) do
              {:ok, jpeg} -> File.write!(Path.join(@output, page.file), jpeg)
              {:error, reason} -> Mix.shell().error("  failed #{page.file}: #{inspect(reason)}")
            end
          end

          Mix.shell().info("Generated #{length(pages)} marketing OG images.")
        after
          OgImageRenderer.stop(renderer)
        end

      {:error, reason} ->
        Mix.shell().error(
          "[marketing.gen.og_images] Chrome unavailable (#{inspect(reason)}); skipping."
        )
    end
  end

  @doc false
  def pages do
    changelog_pages =
      Enum.map(Changelog.entries(), fn entry ->
        %{
          file: "changelog-#{entry.slug}.jpg",
          title: entry.title,
          description: entry.summary,
          subtitle: "Changelog",
          category: ChangelogHTML.format_date(entry.date)
        }
      end)

    blog_pages =
      Enum.map(Blog.all_posts(), fn post ->
        %{
          file: "blog-#{post.slug}.jpg",
          title: post.title,
          subtitle: "Blog"
        }
      end)

    @static_pages ++ changelog_pages ++ blog_pages
  end
end
