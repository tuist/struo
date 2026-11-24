defmodule StruoSiteWeb.PageHTML do
  @moduledoc """
  This module contains pages rendered by PageController.

  See the `page_html` directory for all templates available.
  """
  use StruoSiteWeb, :html

  embed_templates "page_html/*"

  @icons_path Path.join([File.cwd!(), "deps", "noora", "lib", "noora", "icons"])
  @icon_package @icons_path |> Path.join("package.svg") |> File.read!() |> String.trim()
  @icon_reload @icons_path |> Path.join("reload.svg") |> File.read!() |> String.trim()
  @icon_server @icons_path |> Path.join("server.svg") |> File.read!() |> String.trim()
  @icon_chevron @icons_path |> Path.join("chevron-right.svg") |> File.read!() |> String.trim()
  @icon_github @icons_path |> Path.join("brand-github.svg") |> File.read!() |> String.trim()
  @icon_copy @icons_path |> Path.join("copy.svg") |> File.read!() |> String.trim()
  @icon_check @icons_path |> Path.join("check.svg") |> File.read!() |> String.trim()
  @icon_chevron_down @icons_path
                     |> Path.join("chevron-down.svg")
                     |> File.read!()
                     |> String.trim()

  @doc false
  def github_icon, do: @icon_github

  @doc false
  def chevron_icon, do: @icon_chevron

  @doc false
  def copy_icon, do: @icon_copy

  @doc false
  def check_icon, do: @icon_check

  @doc false
  def chevron_down_icon, do: @icon_chevron_down

  @doc """
  The prompt visitors hand to their coding agent to find what Struo cannot
  build in their project yet. `site_url` is where this site is served, so the
  prompt points at the documentation that is live there.

  The page shows `gaps_prompt_lead/0` and keeps `gaps_prompt_steps/1` folded
  until the visitor asks for more.
  """
  def gaps_prompt(site_url), do: gaps_prompt_lead() <> "\n\n" <> gaps_prompt_steps(site_url)

  @doc false
  def gaps_prompt_lead do
    """
    I want to find out how much of this project Struo can build today, and where it falls short.

    Struo is an open source build system (https://github.com/tuist/struo). It reads the build graph from a project's native files, and it does not cover every graph feature yet. Help me find the gaps in this project.
    """
    |> String.trim()
  end

  @doc false
  def gaps_prompt_steps(site_url) do
    """
    1. Build Struo from the latest sources instead of installing a release, so we test what is on main. Clone it outside this project and follow "Build From Source" at #{site_url}/docs/guide/getting-started#build-from-source.
    2. From the root of this project, run the binary you built: `struo query targets`, then `struo build`, then `struo test`. Do not add Struo configuration, and do not change the project to make a command pass.
    3. Compare the results with the project's own build. Note targets Struo did not discover, targets that fail or are skipped, dependencies or settings it did not model, and outputs that differ.
    4. Report what worked, then each gap with the command, its output, and the smallest part of the project that reproduces it. Check https://github.com/tuist/struo/issues for an existing report, and draft an issue for each gap that has none. Show me the drafts before you file anything.
    """
    |> String.trim()
  end

  @doc false
  def features do
    [
      %{
        title: "Build struo",
        icon: @icon_package,
        link: "/docs/guide/scripted/caching",
        details:
          "Declare inputs, outputs, and environment. An action only runs again when one of them changes."
      },
      %{
        title: "Reuse everywhere",
        icon: @icon_reload,
        link: "/docs/guide/scripted",
        details:
          "Restore results from a local or shared cache. Same inputs, same result, no rebuild."
      },
      %{
        title: "Run anywhere",
        icon: @icon_server,
        link: "/docs/guide/infrastructure/remote-execution",
        details:
          "Run actions in a fresh local or hosted sandbox. Declared inputs go in, declared outputs come out."
      }
    ]
  end

  @doc false
  def channels do
    [
      %{label: "Rust", icon: "rust", command: "struo build crates/engine"},
      %{label: "iOS", icon: "apple", command: "struo build apps/ios/App"},
      %{label: "Android", icon: "android", command: "struo build apps/android/App"},
      %{label: "Zig", icon: "zig", command: "struo build zig/app"}
    ]
  end

  # The rotary channel dial, switched off for now.
  # @dial_first_degrees -60
  # @dial_last_degrees 60
  #
  # @doc """
  # Where a channel sits around the dial, in degrees clockwise from twelve o'clock.
  # """
  # def dial_angle(index) do
  #   steps = max(length(channels()) - 1, 1)
  #
  #   Float.round(
  #     @dial_first_degrees + index * (@dial_last_degrees - @dial_first_degrees) / steps,
  #     1
  #   )
  # end

  @doc false
  def languages do
    [
      %{name: "Swift", icon: "swift", link: "/docs/guide/graph/swift-packages"},
      %{name: "Apple", icon: "apple", link: "/docs/guide/graph/apple"},
      %{name: "Kotlin", icon: "kotlin", link: "/docs/guide/graph/kotlin"},
      %{name: "Android", icon: "android", link: "/docs/guide/graph/android"},
      %{name: "Rust", icon: "rust", link: "/docs/guide/graph/rust"},
      %{name: "Go", icon: "go", link: "/docs/guide/graph/go"},
      %{name: "C / C++", icon: "cplusplus", link: "/docs/guide/graph/c"},
      %{name: "Elixir", icon: "elixir", link: "/docs/guide/graph/elixir"},
      %{name: "Ruby", icon: "ruby", link: "/docs/guide/sdk/ruby"},
      %{name: "JavaScript", icon: "javascript", link: "/docs/guide/sdk/javascript"},
      %{name: "Zig", icon: "zig", link: "/docs/guide/graph/zig"},
      %{name: "React Native", icon: "react-native", link: "/docs/guide/graph/react-native"}
    ]
  end
end
