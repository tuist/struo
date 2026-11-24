defmodule StruoSiteWeb.LlmsControllerTest do
  use StruoSiteWeb.ConnCase

  test "GET /llms.txt indexes docs pages as Markdown, grouped like the sidebar", %{conn: conn} do
    body = conn |> get(~p"/llms.txt") |> response(200)
    site = StruoSiteWeb.Endpoint.url()

    assert body =~ ~r/\A# Struo\n\n> /
    assert body =~ "## Scripted Automation\n"
    assert body =~ "- [Getting Started](#{site}/docs-markdown/guide/getting-started): "
    assert body =~ "(#{site}/docs-markdown/reference/prelude/rust_library)"

    {getting_started, _} = :binary.match(body, "/guide/getting-started)")
    {providers, _} = :binary.match(body, "## Infrastructure Providers")
    assert getting_started < providers
  end
end
