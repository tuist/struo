def _jvm_test_runner_source(junit4 = False, junit5 = False):
    source = """import java.io.PrintWriter;
import java.io.StringWriter;
import java.lang.annotation.Annotation;
import java.lang.reflect.AnnotatedElement;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;
import java.util.stream.Stream;

public final class StruoJvmTestRunner {
  private static final class TestCase {
    String id;
    String name;
    String suite;
    String status;
    String failure;
    long durationMs;

    TestCase(String id, String name, String suite) {
      this.id = id;
      this.name = name;
      this.suite = suite;
      this.status = "unknown";
      this.failure = "";
      this.durationMs = -1;
    }
  }

  public static void main(String[] args) throws Exception {
    if (args.length < 6) {
      System.err.println("usage: StruoJvmTestRunner <classes> <results> <log> <native-results> <target> <runner-type>");
      System.exit(2);
    }
    Path classes = Paths.get(args[0]);
    Path results = Paths.get(args[1]);
    Path log = Paths.get(args[2]);
    Path nativeResults = Paths.get(args[3]);
    String target = args[4];
    String runnerType = args[5];
    createParent(results);
    createParent(log);
    createParent(nativeResults);

    String inventory = System.getenv("STRUO_TEST_INVENTORY");
    if (inventory != null) Files.deleteIfExists(Paths.get(inventory));
    StringBuilder logText = new StringBuilder();
    List<String> filters = new ArrayList<>();
    for (int i = 6; i < args.length; i++) {
      filters.add(args[i]);
    }
    List<TestCase> cases = runTests(classes, target, filters, logText);
    if (inventory != null) {
      java.util.Set<String> units = new java.util.TreeSet<>();
      for (TestCase testCase : cases) {
        if ("failed".equals(testCase.status) || !units.add(testCase.id)) {
          System.err.println("ambiguous or unresolved test unit: " + testCase.id);
          System.exit(2);
        }
      }
      StringBuilder encoded = new StringBuilder();
      for (String unit : units) {
        if (encoded.length() > 0) encoded.append(',');
        encoded.append(jsonString(unit));
      }
      Files.writeString(Paths.get(inventory), "{\\"schema\\":\\"struo.test_inventory.v1\\",\\"target\\":" + jsonString(target) + ",\\"units\\":[" + encoded + "]}", StandardCharsets.UTF_8);
      return;
    }
    if (cases.isEmpty()) {
      String reason = filters.isEmpty()
          ? "no test classes found under " + classes
          : "no test matched the filters: " + String.join(", ", filters);
      logText.append(reason).append(System.lineSeparator());
      TestCase testCase = new TestCase(target + "::no-tests", "no-tests", target);
      testCase.status = "failed";
      testCase.failure = reason;
      cases.add(testCase);
    }
    int failed = 0;
    int passed = 0;
    for (TestCase testCase : cases) {
      if ("passed".equals(testCase.status)) {
        passed++;
      } else if ("failed".equals(testCase.status)) {
        failed++;
      }
    }
    String report = reportJson(target, runnerType, cases, passed, failed, log.toString(), nativeResults.toString());
    Files.writeString(log, logText.toString(), StandardCharsets.UTF_8);
    Files.writeString(nativeResults, logText.toString(), StandardCharsets.UTF_8);
    Files.writeString(results, report, StandardCharsets.UTF_8);
    System.exit(failed == 0 ? 0 : 1);
  }

  private static List<TestCase> runTests(Path classes, String target, List<String> filters, StringBuilder logText) throws Exception {
    List<String> classNames = classNames(classes);
    List<TestCase> cases = new ArrayList<>();
    for (String className : classNames) {
      Class<?> type;
      try {
        type = Class.forName(className, false, StruoJvmTestRunner.class.getClassLoader());
      } catch (Throwable error) {
        TestCase testCase = new TestCase(target + "::" + className, className, className);
        testCase.status = "failed";
        testCase.failure = stackTrace(error);
        cases.add(testCase);
        logText.append(testCase.failure);
        continue;
      }
      if (type.isAnonymousClass() || type.isLocalClass() || type.isSynthetic()
          || type.isInterface() || Modifier.isAbstract(type.getModifiers())
          || (type.isMemberClass() && !Modifier.isStatic(type.getModifiers()))) {
        continue;
      }
      Method[] methods = type.getDeclaredMethods();
      List<Method> sortedMethods = new ArrayList<>();
      Collections.addAll(sortedMethods, methods);
      for (Method method : type.getMethods()) {
        if (method.getDeclaringClass() != type) {
          sortedMethods.add(method);
        }
      }
      sortedMethods.sort(Comparator.comparing(Method::getName));
      for (Method method : sortedMethods) {
        if (!isTestMethod(method) || !matchesFilter(filters, className, method.getName())) {
          continue;
        }
        TestCase testCase = new TestCase(target + "::" + className + "." + method.getName(), method.getName(), className);
        if (System.getenv("STRUO_TEST_UNIT") != null && !testCase.id.equals(System.getenv("STRUO_TEST_UNIT"))) continue;
        if (System.getenv("STRUO_TEST_INVENTORY") != null) {
          cases.add(testCase);
          continue;
        }
        if (isIgnored(type) || isIgnored(method)) {
          testCase.status = "skipped";
          logText.append("skipped ").append(className).append(".").append(method.getName()).append(System.lineSeparator());
          cases.add(testCase);
          continue;
        }
        long started = System.nanoTime();
        try {
          invoke(type, method);
          testCase.durationMs = (System.nanoTime() - started) / 1_000_000L;
          testCase.status = "passed";
          logText.append("passed ").append(className).append(".").append(method.getName()).append(System.lineSeparator());
        } catch (Throwable error) {
          testCase.durationMs = (System.nanoTime() - started) / 1_000_000L;
          testCase.status = "failed";
          testCase.failure = stackTrace(error);
          logText.append("failed ").append(className).append(".").append(method.getName()).append(System.lineSeparator());
          logText.append(testCase.failure);
        }
        cases.add(testCase);
      }
    }
    return cases;
  }

  private static boolean matchesFilter(List<String> filters, String className, String methodName) {
    if (filters.isEmpty()) {
      return true;
    }
    for (String filter : filters) {
      int separator = filter.indexOf('#');
      String filterClass = separator < 0 ? filter : filter.substring(0, separator);
      String filterMethod = separator < 0 ? "" : filter.substring(separator + 1);
      if ((filterClass.isEmpty() || filterClass.equals(className)) &&
          (filterMethod.isEmpty() || filterMethod.equals(methodName))) {
        return true;
      }
    }
    return false;
  }

  private static List<String> classNames(Path root) throws Exception {
    List<String> names = new ArrayList<>();
    if (!Files.isDirectory(root)) {
      return names;
    }
    try (Stream<Path> stream = Files.walk(root)) {
      stream
          .filter(Files::isRegularFile)
          .map(root::relativize)
          .map(Path::toString)
          .filter(path -> path.endsWith(".class"))
          .filter(path -> !path.endsWith("module-info.class"))
          .filter(path -> !path.endsWith("package-info.class"))
          .map(path -> path.substring(0, path.length() - ".class".length()))
          .map(path -> path.replace('/', '.').replace((char)92, '.'))
          .sorted()
          .forEach(names::add);
    }
    return names;
  }

  private static boolean isTestMethod(Method method) {
    if (method.getParameterCount() != 0) {
      return false;
    }
    if (method.getName().startsWith("test")) {
      return true;
    }
    for (Annotation annotation : method.getDeclaredAnnotations()) {
      String name = annotation.annotationType().getName();
      if ("org.junit.Test".equals(name) || "kotlin.test.Test".equals(name)) {
        return true;
      }
    }
    return false;
  }

  private static boolean isIgnored(AnnotatedElement element) {
    for (Annotation annotation : element.getDeclaredAnnotations()) {
      String name = annotation.annotationType().getName();
      if ("org.junit.Ignore".equals(name) || "kotlin.test.Ignore".equals(name)) {
        return true;
      }
    }
    return false;
  }

  private static void invoke(Class<?> type, Method method) throws Throwable {
    method.setAccessible(true);
    Object receiver = null;
    if (!Modifier.isStatic(method.getModifiers())) {
      receiver = instance(type);
    }
    try {
      method.invoke(receiver);
    } catch (InvocationTargetException error) {
      throw error.getTargetException();
    }
  }

  private static Object instance(Class<?> type) throws Exception {
    try {
      Field instance = type.getField("INSTANCE");
      if (Modifier.isStatic(instance.getModifiers())) {
        return instance.get(null);
      }
    } catch (NoSuchFieldException ignored) {
    }
    Constructor<?> constructor = type.getDeclaredConstructor();
    constructor.setAccessible(true);
    return constructor.newInstance();
  }

  private static String stackTrace(Throwable error) {
    StringWriter buffer = new StringWriter();
    error.printStackTrace(new PrintWriter(buffer));
    return buffer.toString();
  }

  private static void createParent(Path path) throws Exception {
    Path parent = path.getParent();
    if (parent != null) {
      Files.createDirectories(parent);
    }
  }

  private static String jsonString(String value) {
    StringBuilder out = new StringBuilder();
    out.append((char)34);
    for (int i = 0; i < value.length(); i++) {
      char ch = value.charAt(i);
      switch (ch) {
        case 34:
          out.append((char)92).append((char)34);
          break;
        case 92:
          out.append((char)92).append((char)92);
          break;
        case 10:
          out.append((char)92).append('n');
          break;
        case 13:
          out.append((char)92).append('r');
          break;
        case 9:
          out.append((char)92).append('t');
          break;
        default:
          if (ch < 32) {
            out.append((char)92).append('u').append(String.format("%04x", (int)ch));
          } else {
            out.append(ch);
          }
      }
    }
    out.append((char)34);
    return out.toString();
  }

  private static void field(StringBuilder out, String name) {
    out.append((char)34).append(name).append((char)34).append(':');
  }

  private static String reportJson(String target, String runnerType, List<TestCase> cases, int passed, int failed, String log, String nativeResults) {
    StringBuilder out = new StringBuilder();
    out.append('{');
    field(out, "schema");
    out.append(jsonString("struo.test_results.v1")).append(',');
    field(out, "target");
    out.append(jsonString(target)).append(',');
    field(out, "runner");
    out.append('{');
    field(out, "type");
    out.append(jsonString(runnerType)).append(',');
    field(out, "metadata");
    out.append("{}},");
    field(out, "status");
    out.append(jsonString(failed == 0 ? "passed" : "failed")).append(',');
    field(out, "summary");
    out.append('{');
    field(out, "total");
    out.append(cases.size()).append(',');
    field(out, "passed");
    out.append(passed).append(',');
    field(out, "failed");
    out.append(failed).append(',');
    field(out, "skipped");
    out.append(cases.size() - passed - failed).append(',');
    field(out, "flaky");
    out.append(0).append("},");
    field(out, "cases");
    out.append('[');
    for (int i = 0; i < cases.size(); i++) {
      if (i > 0) {
        out.append(',');
      }
      TestCase testCase = cases.get(i);
      out.append('{');
      field(out, "id");
      out.append(jsonString(testCase.id)).append(',');
      field(out, "name");
      out.append(jsonString(testCase.name)).append(',');
      field(out, "suite");
      out.append(jsonString(testCase.suite)).append(',');
      field(out, "status");
      out.append(jsonString(testCase.status)).append(',');
      field(out, "attempts");
      out.append("[{");
      field(out, "status");
      out.append(jsonString(testCase.status));
      if (testCase.durationMs >= 0) {
        out.append(',');
        field(out, "duration_ms");
        out.append(testCase.durationMs);
      }
      if ("failed".equals(testCase.status) && !testCase.failure.isEmpty()) {
        String message = testCase.failure.length() > 4000 ? testCase.failure.substring(0, 4000) : testCase.failure;
        out.append(',');
        field(out, "failure");
        out.append('{');
        field(out, "message");
        out.append(jsonString(message)).append('}');
      }
      out.append("}],");
      field(out, "runner_metadata");
      out.append("{}");
      out.append('}');
    }
    out.append("],");
    field(out, "artifacts");
    out.append('{');
    field(out, "logs");
    out.append('[').append(jsonString(log)).append("],");
    field(out, "native_results");
    out.append('[').append(jsonString(nativeResults)).append("]}}");
    out.append(System.lineSeparator());
    return out.toString();
  }
}
"""
    if junit4:
        source = source.replace("runTests(classes, target, filters, logText)", "runJUnit4(classes, target, filters, logText)")
        source = source.replace("  private static boolean matchesFilter(", _jvm_junit4_methods() + "  private static boolean matchesFilter(")
    if junit5:
        source = source.replace("import java.util.stream.Stream;\n", "import java.util.stream.Stream;\n" + _JVM_JUNIT_PLATFORM_IMPORTS)
        source = source.replace("runTests(classes, target, filters, logText)", "runJUnitPlatform(classes, target, filters, logText)")
        source = source.replace("  private static boolean matchesFilter(", _jvm_junit_platform_methods() + "  private static boolean matchesFilter(")
    return source

_JVM_JUNIT_PLATFORM_IMPORTS = """import org.junit.platform.engine.TestExecutionResult;
import org.junit.platform.engine.UniqueId;
import org.junit.platform.engine.support.descriptor.ClassSource;
import org.junit.platform.engine.support.descriptor.MethodSource;
import org.junit.platform.launcher.TestIdentifier;
import org.junit.platform.launcher.TestPlan;
"""

def _jvm_junit_platform_methods():
    return """  private static List<TestCase> runJUnitPlatform(Path classes, String target, List<String> filters, StringBuilder logText) throws Exception {
    List<TestCase> cases = new ArrayList<>();
    String exact = System.getenv("STRUO_TEST_UNIT");
    org.junit.platform.launcher.Launcher launcher = org.junit.platform.launcher.core.LauncherFactory.create();
    TestPlan plan = launcher.discover(org.junit.platform.launcher.core.LauncherDiscoveryRequestBuilder.request()
        .selectors(org.junit.platform.engine.discovery.DiscoverySelectors.selectClasspathRoots(Collections.singleton(classes.toAbsolutePath())))
        .build());
    java.util.Set<String> overloaded = platformOverloads(plan);
    List<UniqueId> selected = filters.isEmpty() ? null : platformFilterSelection(plan, filters, overloaded);
    if (selected != null && selected.isEmpty()) return cases;
    String[] exactUnit = null;
    String exactRoot = null;
    if (exact != null) {
      TestPlan scope = selected == null ? plan : launcher.discover(platformRequest(selected));
      UniqueId root = null;
      for (TestIdentifier id : platformIdentifiers(scope)) {
        if (!isPlatformUnit(scope, id)) continue;
        String[] name = platformUnit(scope, id, overloaded);
        if (exact.equals(target + "::" + name[0] + "." + name[1])) {
          root = UniqueId.parse(id.getUniqueId());
          exactUnit = name;
          break;
        }
      }
      if (root == null) return cases;
      List<UniqueId> narrowed = new ArrayList<>();
      for (UniqueId selector : selected == null ? Collections.singletonList(root) : selected) {
        UniqueId kept = selector.hasPrefix(root) ? selector : root.hasPrefix(selector) ? root : null;
        if (kept != null && !narrowed.contains(kept)) narrowed.add(kept);
      }
      selected = narrowed;
      exactRoot = root.toString();
    }
    if (selected != null) plan = launcher.discover(platformRequest(selected));
    java.util.Map<String, TestCase> byId = new java.util.TreeMap<>();
    if (System.getenv("STRUO_TEST_INVENTORY") != null) {
      for (TestIdentifier id : platformIdentifiers(plan)) {
        if (!isPlatformUnit(plan, id)) continue;
        String[] name = platformUnit(plan, id, overloaded);
        TestCase unit = new TestCase(target + "::" + name[0] + "." + name[1], name[1], name[0]);
        if (byId.put(unit.id, unit) != null) unit.status = "failed";
      }
      for (TestIdentifier id : platformIdentifiers(plan)) {
        if (!id.isContainer() || isPlatformUnit(plan, id) || !plan.getParent(id).isPresent() || !plan.getChildren(id).isEmpty()) continue;
        Object source = id.getSource().orElse(null);
        String suite = source instanceof ClassSource ? ((ClassSource) source).getClassName() : id.getLegacyReportingName();
        TestCase unresolved = new TestCase(target + "::" + suite, suite, suite);
        unresolved.status = "failed";
        byId.put(unresolved.id + "\\u0000" + id.getUniqueId(), unresolved);
      }
      cases.addAll(byId.values());
      return cases;
    }
    java.util.Set<String> reported = new java.util.HashSet<>();
    java.util.Map<String, Long> started = new java.util.HashMap<>();
    String[] rootOutcome = new String[1];
    String rootId = exactRoot;
    launcher.execute(plan, new org.junit.platform.launcher.TestExecutionListener() {
      TestPlan current;

      @Override public synchronized void testPlanExecutionStarted(TestPlan testPlan) {
        current = testPlan;
      }

      private void record(TestIdentifier id, String suite, String name, String status, String failure) {
        reported.add(id.getUniqueId());
        TestCase result = byId.computeIfAbsent(target + "::" + suite + "." + name, key -> new TestCase(key, name, suite));
        if (!"failed".equals(result.status) && !("passed".equals(result.status) && "skipped".equals(status))) result.status = status;
        result.failure += failure;
        Long start = started.remove(id.getUniqueId());
        if (start != null) result.durationMs = Math.max(result.durationMs, 0) + (System.nanoTime() - start) / 1_000_000L;
      }

      private void record(TestIdentifier id, String status, String failure) {
        String[] name = platformCase(current, id, overloaded);
        record(id, name[0], name[1], status, failure);
      }

      private void skipUnreported(TestIdentifier id) {
        if (reported.contains(id.getUniqueId())) return;
        java.util.Set<TestIdentifier> children = current.getChildren(id);
        if (id.isTest() || (isPlatformUnit(current, id) && children.isEmpty())) {
          record(id, "skipped", "");
          return;
        }
        for (TestIdentifier child : children) skipUnreported(child);
      }

      @Override public synchronized void executionStarted(TestIdentifier id) {
        if (id.isTest()) started.put(id.getUniqueId(), System.nanoTime());
      }

      @Override public synchronized void executionSkipped(TestIdentifier id, String reason) {
        skipUnreported(id);
      }

      @Override public synchronized void executionFinished(TestIdentifier id, TestExecutionResult result) {
        TestExecutionResult.Status outcome = result.getStatus();
        if (id.getUniqueId().equals(rootId)) rootOutcome[0] = outcome.name();
        String failure = result.getThrowable().map(StruoJvmTestRunner::stackTrace).orElse("");
        if (id.isTest()) {
          String status = outcome == TestExecutionResult.Status.SUCCESSFUL ? "passed"
              : outcome == TestExecutionResult.Status.ABORTED ? "skipped" : "failed";
          record(id, status, outcome == TestExecutionResult.Status.FAILED ? failure : "");
          return;
        }
        if (outcome == TestExecutionResult.Status.ABORTED) {
          skipUnreported(id);
        } else if (outcome == TestExecutionResult.Status.FAILED) {
          if (isPlatformUnit(current, id)) {
            record(id, "failed", failure);
          } else {
            Object source = id.getSource().orElse(null);
            String suite = source instanceof ClassSource ? ((ClassSource) source).getClassName() : id.getLegacyReportingName();
            record(id, suite, "classMethod", "failed", failure);
          }
        }
      }
    });
    if (exactUnit != null) {
      if (byId.isEmpty() && rootOutcome[0] == null) return cases;
      TestCase unit = new TestCase(exact, exactUnit[1], exactUnit[0]);
      unit.status = byId.isEmpty() ? ("SUCCESSFUL".equals(rootOutcome[0]) ? "passed" : "failed") : "skipped";
      for (TestCase result : byId.values()) {
        boolean classLevel = "classMethod".equals(result.name) && byId.size() > 1;
        if (!classLevel && ("failed".equals(result.status) || ("passed".equals(result.status) && !"failed".equals(unit.status)))) unit.status = result.status;
        unit.failure += result.failure;
        if (result.durationMs >= 0) unit.durationMs = Math.max(unit.durationMs, 0) + result.durationMs;
      }
      cases.add(unit);
    } else {
      cases.addAll(byId.values());
    }
    for (TestCase result : cases) {
      logText.append(result.status).append(" ").append(result.suite).append(".").append(result.name).append(System.lineSeparator());
      if (!result.failure.isEmpty()) logText.append(result.failure);
    }
    return cases;
  }

  private static org.junit.platform.launcher.LauncherDiscoveryRequest platformRequest(List<UniqueId> ids) {
    List<org.junit.platform.engine.DiscoverySelector> selectors = new ArrayList<>();
    for (UniqueId id : ids) selectors.add(org.junit.platform.engine.discovery.DiscoverySelectors.selectUniqueId(id));
    return org.junit.platform.launcher.core.LauncherDiscoveryRequestBuilder.request().selectors(selectors).build();
  }

  private static List<UniqueId> platformFilterSelection(TestPlan plan, List<String> filters, java.util.Set<String> overloaded) {
    List<UniqueId> selected = new ArrayList<>();
    for (TestIdentifier id : platformIdentifiers(plan)) {
      Object source = id.getSource().orElse(null);
      boolean unit = isPlatformUnit(plan, id);
      String[] name = unit ? platformUnit(plan, id, overloaded) : null;
      for (String filter : filters) {
        int separator = filter.indexOf('#');
        String filterClass = separator < 0 ? filter : filter.substring(0, separator);
        String filterMethod = separator < 0 ? "" : filter.substring(separator + 1);
        List<UniqueId> matches = new ArrayList<>();
        if (filterMethod.isEmpty()) {
          if (source instanceof ClassSource) {
            String className = ((ClassSource) source).getClassName();
            if (className.equals(filterClass) || className.startsWith(filterClass + "$")) matches.add(UniqueId.parse(id.getUniqueId()));
          } else if (unit && name[0].equals(filterClass)) {
            matches.add(UniqueId.parse(id.getUniqueId()));
          }
        } else if (unit && (filterClass.isEmpty() || filterClass.equals(name[0]))) {
          matches = platformMethodMatches(id, name[1], filterMethod);
        }
        for (UniqueId match : matches) if (!selected.contains(match)) selected.add(match);
      }
    }
    return selected;
  }

  private static List<UniqueId> platformMethodMatches(TestIdentifier id, String unitName, String filterMethod) {
    List<UniqueId> out = new ArrayList<>();
    UniqueId uniqueId = UniqueId.parse(id.getUniqueId());
    String method = isPlatformMethod(id) ? ((MethodSource) id.getSource().get()).getMethodName() : unitName;
    if (filterMethod.equals(unitName) || filterMethod.equals(method)) {
      out.add(uniqueId);
      return out;
    }
    int bracket = filterMethod.indexOf('[');
    if (bracket <= 0 || !isPlatformMethod(id) || unitName.contains("[")) return out;
    String base = filterMethod.substring(0, bracket);
    if (!base.equals(unitName) && !base.equals(method)) return out;
    String type = uniqueId.getLastSegment().getType();
    if (!"test-template".equals(type) && !"test-factory".equals(type)) return out;
    List<String> indexes = new ArrayList<>();
    for (int at = bracket; at < filterMethod.length();) {
      int close = filterMethod.indexOf(']', at);
      if (filterMethod.charAt(at) != '[' || close < 0) return out;
      indexes.add(filterMethod.substring(at + 1, close));
      at = close + 1;
    }
    if ("test-template".equals(type) && indexes.size() != 1) return out;
    for (int i = 0; i < indexes.size(); i++) {
      String segment = "test-template".equals(type) ? "test-template-invocation"
          : i + 1 < indexes.size() ? "dynamic-container" : "dynamic-test";
      if (i + 1 == indexes.size() && "test-factory".equals(type)) out.add(uniqueId.append("dynamic-container", "#" + indexes.get(i)));
      uniqueId = uniqueId.append(segment, "#" + indexes.get(i));
    }
    out.add(uniqueId);
    return out;
  }

  private static List<TestIdentifier> platformIdentifiers(TestPlan plan) {
    List<TestIdentifier> out = new ArrayList<>();
    List<TestIdentifier> pending = new ArrayList<>(plan.getRoots());
    while (!pending.isEmpty()) {
      TestIdentifier id = pending.remove(pending.size() - 1);
      out.add(id);
      pending.addAll(plan.getChildren(id));
    }
    return out;
  }

  private static boolean isPlatformMethod(TestIdentifier id) {
    return id.getSource().filter(source -> source instanceof MethodSource).isPresent();
  }

  private static boolean isPlatformUnit(TestPlan plan, TestIdentifier id) {
    if (isPlatformMethod(id)) return !plan.getParent(id).filter(StruoJvmTestRunner::isPlatformMethod).isPresent();
    if (!id.isTest()) return false;
    for (TestIdentifier at = plan.getParent(id).orElse(null); at != null; at = plan.getParent(at).orElse(null)) {
      if (isPlatformMethod(at)) return false;
    }
    return true;
  }

  private static String platformBaseName(TestIdentifier id) {
    String method = ((MethodSource) id.getSource().get()).getMethodName();
    String legacy = id.getLegacyReportingName();
    return legacy.startsWith(method + "[") ? legacy : method;
  }

  private static java.util.Set<String> platformOverloads(TestPlan plan) {
    java.util.Map<String, java.util.Set<String>> parameters = new java.util.HashMap<>();
    for (TestIdentifier id : platformIdentifiers(plan)) {
      if (!isPlatformMethod(id) || !isPlatformUnit(plan, id)) continue;
      MethodSource source = (MethodSource) id.getSource().get();
      parameters.computeIfAbsent(source.getClassName() + "#" + platformBaseName(id), key -> new java.util.HashSet<>())
          .add(source.getMethodParameterTypes());
    }
    java.util.Set<String> out = new java.util.HashSet<>();
    for (java.util.Map.Entry<String, java.util.Set<String>> entry : parameters.entrySet()) {
      if (entry.getValue().size() > 1) out.add(entry.getKey());
    }
    return out;
  }

  private static String[] platformUnit(TestPlan plan, TestIdentifier id, java.util.Set<String> overloaded) {
    if (isPlatformMethod(id)) {
      MethodSource source = (MethodSource) id.getSource().get();
      String name = platformBaseName(id);
      if (overloaded.contains(source.getClassName() + "#" + name)) name += "(" + source.getMethodParameterTypes() + ")";
      return new String[] {source.getClassName(), name};
    }
    List<TestIdentifier> chain = new ArrayList<>();
    for (TestIdentifier at = id; at != null; at = plan.getParent(at).orElse(null)) chain.add(at);
    if (chain.size() > 1) chain.remove(chain.size() - 1);
    int top = -1;
    for (int i = 0; i < chain.size(); i++) {
      if (chain.get(i).getSource().filter(source -> source instanceof ClassSource).isPresent()) top = i;
    }
    String suite;
    if (top >= 0) {
      suite = ((ClassSource) chain.get(top).getSource().get()).getClassName();
    } else {
      top = chain.size() - 1;
      suite = chain.get(top).getLegacyReportingName();
    }
    StringBuilder name = new StringBuilder();
    for (int i = top - 1; i >= 0; i--) {
      if (name.length() > 0) name.append('/');
      name.append(chain.get(i).getLegacyReportingName());
    }
    return new String[] {suite, name.length() == 0 ? id.getLegacyReportingName() : name.toString()};
  }

  private static String[] platformCase(TestPlan plan, TestIdentifier id, java.util.Set<String> overloaded) {
    TestIdentifier root = null;
    for (TestIdentifier at = id; at != null; at = plan.getParent(at).orElse(null)) {
      if (isPlatformMethod(at)) root = at;
    }
    if (root == null) return platformUnit(plan, id, overloaded);
    String[] unit = platformUnit(plan, root, overloaded);
    StringBuilder name = new StringBuilder(unit[1]);
    List<UniqueId.Segment> rootSegments = UniqueId.parse(root.getUniqueId()).getSegments();
    List<UniqueId.Segment> segments = UniqueId.parse(id.getUniqueId()).getSegments();
    for (int i = rootSegments.size(); i < segments.size(); i++) {
      String value = segments.get(i).getValue();
      name.append('[').append(value.startsWith("#") ? value.substring(1) : value).append(']');
    }
    return new String[] {unit[0], name.toString()};
  }

"""

def _jvm_junit4_methods():
    return """  private static List<TestCase> runJUnit4(Path classes, String target, List<String> filters, StringBuilder logText) throws Exception {
    List<TestCase> cases = new ArrayList<>();
    org.junit.runner.JUnitCore core = new org.junit.runner.JUnitCore();
    core.addListener(new org.junit.runner.notification.RunListener() {
      final java.util.Map<org.junit.runner.Description, TestCase> active = new java.util.HashMap<>();
      final java.util.Map<org.junit.runner.Description, Long> started = new java.util.HashMap<>();

      private TestCase entry(org.junit.runner.Description description) {
        TestCase result = active.get(description);
        if (result == null) {
          String suite = description.getClassName();
          String name = description.getMethodName();
          if (suite == null) suite = description.getDisplayName();
          if (name == null) name = description.getDisplayName();
          result = new TestCase(target + "::" + suite + "." + name, name, suite);
          active.put(description, result);
          cases.add(result);
        }
        return result;
      }

      @Override public void testStarted(org.junit.runner.Description description) {
        TestCase result = entry(description);
        if (!"failed".equals(result.status)) result.status = "passed";
        started.put(description, System.nanoTime());
      }

      @Override public void testFailure(org.junit.runner.notification.Failure failure) {
        TestCase result = entry(failure.getDescription());
        result.status = "failed";
        result.failure += failure.getTrace();
      }

      @Override public void testAssumptionFailure(org.junit.runner.notification.Failure failure) {
        TestCase result = entry(failure.getDescription());
        if (!"failed".equals(result.status)) result.status = "skipped";
      }

      @Override public void testIgnored(org.junit.runner.Description description) {
        if (description.getMethodName() == null && description.getClassName() != null) {
          List<org.junit.runner.Description> children = new ArrayList<>();
          collectIgnoredClassCases(ignoredClassDescription(description.getClassName()), filters, children);
          if (!children.isEmpty()) {
            for (org.junit.runner.Description child : children) testIgnored(child);
            return;
          }
        }
        if (System.getenv("STRUO_TEST_UNIT") != null && !(target + "::" + description.getClassName() + "." + description.getMethodName()).equals(System.getenv("STRUO_TEST_UNIT"))) return;
        TestCase result = entry(description);
        if (!"failed".equals(result.status)) result.status = "skipped";
      }

      @Override public void testFinished(org.junit.runner.Description description) {
        TestCase result = entry(description);
        Long start = started.remove(description);
        if (start != null) result.durationMs = (System.nanoTime() - start) / 1_000_000L;
      }
    });
    java.util.Map<String, org.junit.runner.Runner> runners = new java.util.LinkedHashMap<>();
    for (String className : classNames(classes)) {
      if (!filters.isEmpty()) {
        boolean selected = false;
        for (String filter : filters) {
          String filterClass = filter.split("#", -1)[0];
          if (filterClass.isEmpty() || filterClass.equals(className)) selected = true;
        }
        if (!selected) continue;
      }
      Class<?> type = Class.forName(className, false, StruoJvmTestRunner.class.getClassLoader());
      if (type.isAnonymousClass() || type.isLocalClass() || type.isSynthetic()
          || type.isInterface() || Modifier.isAbstract(type.getModifiers())
          || (type.isMemberClass() && !Modifier.isStatic(type.getModifiers()))) continue;
      boolean suite = type.isAnnotationPresent(org.junit.runner.RunWith.class);
      for (Class<?> base = type; base != null; base = base.getSuperclass()) {
        for (Method method : base.getDeclaredMethods()) {
          if (method.isAnnotationPresent(org.junit.Test.class)) suite = true;
        }
      }
      if (!suite) continue;
      org.junit.runner.Request request = org.junit.runner.Request.aClass(type);
      if (!filters.isEmpty() && !matchesFilter(filters, className, "")) {
        org.junit.runner.manipulation.Filter filter = new org.junit.runner.manipulation.Filter() {
          @Override public boolean shouldRun(org.junit.runner.Description description) {
            if (description.isTest()) {
              String method = description.getMethodName();
              return matchesFilter(filters, description.getClassName(), method == null ? "" : method)
                  || (method != null && method.contains("[")
                      && matchesFilter(filters, description.getClassName(), method.substring(0, method.indexOf('['))));
            }
            for (org.junit.runner.Description child : description.getChildren()) {
              if (shouldRun(child)) return true;
            }
            return false;
          }
          @Override public String describe() { return String.join(", ", filters); }
        };
        org.junit.runner.Description described = request.getRunner().getDescription();
        if (described.getChildren().isEmpty() && type.isAnnotationPresent(org.junit.Ignore.class)) {
          org.junit.runner.Description expanded = ignoredClassDescription(className);
          if (expanded != null) described = expanded;
        }
        if (!filter.shouldRun(described)) continue;
        request = request.filterWith(filter);
      }
      String exact = System.getenv("STRUO_TEST_UNIT");
      if (exact != null) {
        org.junit.runner.manipulation.Filter filter = new org.junit.runner.manipulation.Filter() {
          @Override public boolean shouldRun(org.junit.runner.Description description) {
            if (description.isTest() && description.getMethodName() != null) return exact.equals(target + "::" + description.getClassName() + "." + description.getMethodName());
            for (org.junit.runner.Description child : description.getChildren()) if (shouldRun(child)) return true;
            return false;
          }
          @Override public String describe() { return exact; }
        };
        org.junit.runner.Description described = request.getRunner().getDescription();
        if (described.getChildren().isEmpty() && type.isAnnotationPresent(org.junit.Ignore.class)) {
          org.junit.runner.Description expanded = ignoredClassDescription(className);
          if (expanded != null) described = expanded;
        }
        if (!filter.shouldRun(described)) continue;
        request = request.filterWith(filter);
      }
      runners.put(className, request.getRunner());
    }
    java.util.Set<String> owned = new java.util.HashSet<>();
    for (java.util.Map.Entry<String, org.junit.runner.Runner> runner : runners.entrySet()) {
      collectJUnit4OwnedClasses(runner.getValue().getDescription(), runner.getKey(), owned);
    }
    for (java.util.Map.Entry<String, org.junit.runner.Runner> runner : runners.entrySet()) {
      if (!owned.contains(runner.getKey())) {
        if (System.getenv("STRUO_TEST_INVENTORY") != null) {
          collectJUnit4Inventory(runner.getValue().getDescription(), target, cases);
        } else core.run(runner.getValue());
      }
    }
    for (TestCase result : cases) {
      logText.append(result.status).append(" ").append(result.suite).append(".").append(result.name).append(System.lineSeparator());
      if (!result.failure.isEmpty()) logText.append(result.failure);
    }
    return cases;
  }

  private static void collectJUnit4Inventory(org.junit.runner.Description description, String target, List<TestCase> out) {
    if (description.isTest() && description.getClassName() != null && description.getMethodName() != null) {
      out.add(new TestCase(target + "::" + description.getClassName() + "." + description.getMethodName(), description.getMethodName(), description.getClassName()));
    } else if (description.getChildren().isEmpty() && description.getClassName() != null) {
      org.junit.runner.Description expanded = ignoredClassDescription(description.getClassName());
      if (expanded != null && !expanded.getChildren().isEmpty()) collectJUnit4Inventory(expanded, target, out);
      else {
        TestCase unresolved = new TestCase(target + "::" + description.getClassName(), description.getDisplayName(), description.getClassName());
        unresolved.status = "failed";
        out.add(unresolved);
      }
    } else {
      for (org.junit.runner.Description child : description.getChildren()) collectJUnit4Inventory(child, target, out);
    }
  }

  private static org.junit.runner.Description ignoredClassDescription(String className) {
    try {
      Class<?> type = Class.forName(className, false, StruoJvmTestRunner.class.getClassLoader());
      org.junit.runner.Runner runner = new org.junit.internal.builders.AllDefaultPossibilitiesBuilder(true) {
        @Override protected org.junit.internal.builders.IgnoredBuilder ignoredBuilder() {
          return new org.junit.internal.builders.IgnoredBuilder() {
            @Override public org.junit.runner.Runner runnerForClass(Class<?> testClass) { return null; }
          };
        }
      }.runnerForClass(type);
      return runner == null ? null : runner.getDescription();
    } catch (Throwable error) {
      return null;
    }
  }

  private static void collectIgnoredClassCases(org.junit.runner.Description description, List<String> filters, List<org.junit.runner.Description> out) {
    if (description == null) return;
    if (description.isTest()) {
      String method = description.getMethodName();
      if (method == null) return;
      if (filters.isEmpty() || matchesFilter(filters, description.getClassName(), method)
          || (method.contains("[") && matchesFilter(filters, description.getClassName(), method.substring(0, method.indexOf('['))))) {
        out.add(description);
      }
      return;
    }
    for (org.junit.runner.Description child : description.getChildren()) {
      collectIgnoredClassCases(child, filters, out);
    }
  }

  private static void collectJUnit4OwnedClasses(org.junit.runner.Description description, String owner, java.util.Set<String> owned) {
    String name = description.getClassName();
    if (name != null && !name.equals(owner)) owned.add(name);
    for (org.junit.runner.Description child : description.getChildren()) {
      collectJUnit4OwnedClasses(child, owner, owned);
    }
  }

"""
