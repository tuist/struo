def _ruby_coverage_hook():
    return '''require "coverage"
require "json"

if encoded = ENV.delete("STRUO_RUBY_COVERAGE")
  config = JSON.parse(encoded)
  root = File.realpath(Dir.pwd)
  spawned = false
  tracer = TracePoint.new(:c_call) do |event|
    spawned = true if [:spawn, :system, :fork, :exec, :popen, :daemon, :`].include?(event.method_id)
  end
  Coverage.start(lines: true)
  tracer.enable
  at_exit do
    tracer.disable
    complete = !spawned
    files = []
    begin
      measured = Coverage.result
      config.fetch("sources").sort.uniq.each do |source|
        absolute = File.realpath(source)
        raise "coverage source outside workspace" unless absolute.start_with?(root + File::SEPARATOR)
        counters = measured.fetch(absolute, {}).fetch(:lines, nil) || Coverage.line_stub(absolute)
        numbers = []
        counts = []
        counters.each_with_index do |count, index|
          next if count.nil?
          numbers << index + 1
          counts << count
        end
        unless numbers.empty?
          files << {"path" => absolute.delete_prefix(root + File::SEPARATOR).tr(File::SEPARATOR, "/"),
                    "line_numbers" => numbers, "execution_counts" => counts}
        end
      end
    rescue Exception => error
      warn "incomplete Ruby coverage: #{error}"
      complete = false
    end
    File.write(config.fetch("output"), JSON.generate({"tool" => "ruby-coverage", "tool_version" => RUBY_VERSION,
               "platform" => RUBY_PLATFORM, "partial" => !complete, "files" => files}))
  end
end
'''

def _ruby_coverage_adapter_support():
    return '''require "json"
require "shellwords"

module StruoCoverage
  def self.configure(parser, options)
    options[:coverage_sources] = []
    parser.on("--struo-coverage-hook PATH") { |value| options[:coverage_hook] = value }
    parser.on("--struo-coverage-source PATH") { |value| options[:coverage_sources] << value }
    parser.on("--struo-inventory") { options[:inventory] = true }
  end

  def self.environment(options)
    return {} unless options[:coverage_hook]
    profile = options[:results] + ".coverage.json"
    File.delete(profile) if File.exist?(profile)
    {"RUBYOPT" => [ENV["RUBYOPT"], "-r" + File.expand_path(options[:coverage_hook]).shellescape].compact.join(" "),
     "STRUO_RUBY_COVERAGE" => JSON.generate({"sources" => options[:coverage_sources], "output" => profile})}
  end

  def self.attach(normalized, options)
    return unless options[:coverage_hook]
    profile = options[:results] + ".coverage.json"
    if File.exist?(profile)
      normalized["coverage"] = JSON.parse(File.read(profile))
      normalized["coverage"]["partial"] ||= normalized["status"] != "passed" || normalized["cases"].length != 1
    end
    normalized["artifacts"]["logs"] = [File.join(File.dirname(options[:results]), "runner.log")]
  end
end
'''
