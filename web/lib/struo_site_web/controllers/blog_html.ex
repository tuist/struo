defmodule StruoSiteWeb.BlogHTML do
  @moduledoc """
  Renders the blog.
  """

  use StruoSiteWeb, :html

  alias StruoSite.Blog.Authors

  embed_templates "blog_html/*"

  @doc false
  def format_date(date), do: Calendar.strftime(date, "%B %-d, %Y")

  @doc false
  def author_names(authors), do: authors |> Enum.map_join(", ", & &1.name)

  @doc false
  def avatar_url(author, size \\ 96), do: Authors.avatar_url(author, size)
end
