defmodule StruoSiteWeb.PageController do
  use StruoSiteWeb, :controller

  def home(conn, _params) do
    conn
    |> assign(:page_title, "Remote cache & execution, for the rest of us")
    |> assign(:meta_description, meta_description())
    |> assign(:og_image, "/images/og/home.jpg")
    |> render(:home)
  end

  @moved_pages %{
    "/docs/guide/infrastructure/connect" =>
      "/docs/guide/infrastructure/tuist#create-and-connect-a-project"
  }

  def moved(conn, _params) do
    conn
    |> put_status(:moved_permanently)
    |> redirect(to: Map.fetch!(@moved_pages, conn.request_path))
  end

  defp meta_description do
    "Struo gives every action explicit inputs, outputs, and environment, so results are " <>
      "content-addressed, cached, and reusable across developers, coding agents, CI, and machines."
  end
end
