defmodule StruoSiteWeb.SitemapController do
  @moduledoc """
  Serves `/sitemap.xml`: the home page, every documentation page, the blog and
  its posts, and the changelog and its entries.
  """
  use StruoSiteWeb, :controller

  alias StruoSite.Blog
  alias StruoSite.Changelog
  alias StruoSiteWeb.Docs
  alias StruoSiteWeb.Endpoint

  def index(conn, _params) do
    conn
    |> put_resp_content_type("application/xml")
    |> send_resp(200, sitemap(pages()))
  end

  # Each page is its path and, when the content carries a date, when it last changed.
  defp pages do
    [{"/", nil}, {"/docs", nil}] ++
      Enum.map(Docs.paths(), &{&1, nil}) ++
      [{"/blog", Blog.last_updated()}] ++
      Enum.map(Blog.all_posts(), &{"/blog/" <> &1.slug, &1.date}) ++
      [{"/changelog", Changelog.last_updated()}] ++
      Enum.map(Changelog.entries(), &{"/changelog/" <> &1.slug, &1.date})
  end

  defp sitemap(pages) do
    urls =
      Enum.map_join(pages, "\n", fn {path, changed} ->
        location = Plug.HTML.html_escape(Endpoint.url() <> path)
        changed = if changed, do: "<lastmod>#{Date.to_iso8601(changed)}</lastmod>", else: ""
        "  <url><loc>#{location}</loc>#{changed}</url>"
      end)

    """
    <?xml version="1.0" encoding="UTF-8"?>
    <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
    #{urls}
    </urlset>
    """
  end
end
