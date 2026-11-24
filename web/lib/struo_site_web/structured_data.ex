defmodule StruoSiteWeb.StructuredData do
  @moduledoc """
  The schema.org description every page carries in its head. It names Tuist as
  the organization that makes and publishes Struo, so search engines attribute
  the site, the software, and its blog posts to Tuist.
  """

  alias StruoSiteWeb.Endpoint
  alias StruoSiteWeb.Layouts

  @organization_id "https://tuist.dev/#organization"
  @repository "https://github.com/tuist/struo"

  @doc "The JSON-LD graph for a page, as text that is safe inside a script element."
  def json_ld(assigns) do
    site = Endpoint.url()
    graph = [organization(), website(site), software(site)] ++ article(assigns, site)

    Jason.encode!(%{"@context" => "https://schema.org", "@graph" => graph}, escape: :html_safe)
  end

  defp organization do
    %{
      "@type" => "Organization",
      "@id" => @organization_id,
      "name" => "Tuist",
      "url" => "https://tuist.dev",
      "sameAs" => ["https://github.com/tuist"]
    }
  end

  defp website(site) do
    %{
      "@type" => "WebSite",
      "@id" => site <> "/#website",
      "name" => "Struo",
      "alternateName" => "Struo by Tuist",
      "url" => site <> "/",
      "publisher" => %{"@id" => @organization_id}
    }
  end

  defp software(site) do
    %{
      "@type" => "SoftwareApplication",
      "@id" => site <> "/#software",
      "name" => "Struo",
      "url" => site <> "/",
      "description" => Layouts.default_description(),
      "applicationCategory" => "DeveloperApplication",
      "isAccessibleForFree" => true,
      "license" => "https://opensource.org/license/mit",
      "sameAs" => [@repository],
      "author" => %{"@id" => @organization_id},
      "publisher" => %{"@id" => @organization_id}
    }
  end

  defp article(%{og_type: "article", conn: conn} = assigns, site) do
    url = assigns[:canonical_url] || site <> conn.request_path

    authors =
      for author <- assigns[:authors] || [], do: %{"@type" => "Person", "name" => author.name}

    [
      %{
        "@type" => "BlogPosting",
        "@id" => url <> "#article",
        "url" => url,
        "headline" => assigns[:page_title],
        "description" => assigns[:meta_description],
        "datePublished" => assigns[:published_time],
        "author" => if(authors == [], do: %{"@id" => @organization_id}, else: authors),
        "publisher" => %{"@id" => @organization_id},
        "isPartOf" => %{"@id" => site <> "/#website"}
      }
    ]
  end

  defp article(_assigns, _site), do: []
end
