defmodule StruoSiteWeb.SitemapControllerTest do
  use StruoSiteWeb.ConnCase

  alias StruoSiteWeb.Endpoint

  test "GET /sitemap.xml lists the home page, docs, blog posts, and changelog entries", %{
    conn: conn
  } do
    conn = get(conn, ~p"/sitemap.xml")
    body = response(conn, 200)

    assert response_content_type(conn, :xml)

    locations =
      ~r{<loc>([^<]+)</loc>} |> Regex.scan(body, capture: :all_but_first) |> List.flatten()

    assert locations == Enum.uniq(locations)

    for path <- [
          "/",
          "/docs",
          "/docs/guide",
          "/docs/guide/getting-started",
          "/docs/guide/infrastructure/tuist",
          "/docs/reference/manifest",
          "/blog",
          "/blog/keep-your-graph-own-your-build",
          "/changelog",
          "/changelog/2026-09-21-rule-neutral-runtime"
        ] do
      assert (Endpoint.url() <> path) in locations
    end

    assert body =~
             "<loc>#{Endpoint.url()}/blog/keep-your-graph-own-your-build</loc><lastmod>2026-11-19</lastmod>"

    refute Enum.any?(locations, &String.ends_with?(&1, "/index"))
  end

  test "robots.txt points crawlers at the sitemap", %{conn: conn} do
    assert conn |> get("/robots.txt") |> response(200) =~ "Sitemap: https://struo.run/sitemap.xml"
  end
end
