defmodule StruoSiteWeb.Plugs.DocsNotFound do
  @moduledoc """
  Answers a documentation URL that has no page with a 404 status. The docs
  LiveView still renders its "Page not found" page inside the docs shell, but
  a LiveView cannot set the response status itself, and a 200 would let search
  engines index the miss.
  """
  import Plug.Conn

  alias StruoSiteWeb.Docs

  def init(opts), do: opts

  def call(%Plug.Conn{path_info: ["docs", _ | _] = path_info} = conn, _opts) do
    case Docs.source_path(tl(path_info)) do
      {:ok, _file} -> conn
      :error -> put_status(conn, :not_found)
    end
  end

  def call(conn, _opts), do: conn
end
