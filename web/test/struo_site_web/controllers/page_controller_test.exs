defmodule StruoSiteWeb.PageControllerTest do
  use StruoSiteWeb.ConnCase

  test "GET /", %{conn: conn} do
    conn = get(conn, ~p"/")
    response = html_response(conn, 200)

    assert response =~ "<h1>Remote cache &amp; execution,<br>for the rest of us</h1>"

    assert response =~
             ~s(property="og:title" content="Remote cache &amp; execution, for the rest of us")

    assert response =~ "Natively supported"
    assert response =~ "Built with"
    assert response =~ "Help us find the gaps"
    assert response =~ "Build Struo from the latest sources"
    assert response =~ "https://discord.gg/fTpB5e3rRp"
    assert response =~ ~s(aria-label="GitHub")
    assert response =~ ~s(aria-label="Discord")
  end

  test "GET / attributes Struo to Tuist in its structured data", %{conn: conn} do
    response = conn |> get(~p"/") |> html_response(200)

    [_, json] = Regex.run(~r{<script type="application/ld\+json">(.*?)</script>}s, response)
    graph = Map.new(Jason.decode!(json)["@graph"], &{&1["@type"], &1})

    assert %{"name" => "Tuist", "url" => "https://tuist.dev", "@id" => tuist} =
             graph["Organization"]

    assert graph["WebSite"]["publisher"] == %{"@id" => tuist}
    assert graph["SoftwareApplication"]["author"] == %{"@id" => tuist}
    refute graph["BlogPosting"]
    assert response =~ ~s(<meta name="author" content="Tuist">)
  end

  test "permanently redirects the removed Connect a Project guide", %{conn: conn} do
    conn = get(conn, "/docs/guide/infrastructure/connect")

    assert redirected_to(conn, 301) ==
             "/docs/guide/infrastructure/tuist#create-and-connect-a-project"
  end
end
