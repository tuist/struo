_CMAKE_BUILD_DRIVER = """
cmake_minimum_required(VERSION 3.20)

foreach(_struo_required STRUO_SOURCE_DIR STRUO_BUILD_DIR)
  if(NOT DEFINED ${_struo_required} OR "${${_struo_required}}" STREQUAL "")
    message(FATAL_ERROR "${_struo_required} is required")
  endif()
endforeach()

set(_struo_prefix_map_args)
if(DEFINED STRUO_PREFIX_MAP)
  set(_struo_prefix_map_args "-DSTRUO_WORKSPACE_DIR:PATH=${STRUO_WORKSPACE_DIR}" "-DCMAKE_PROJECT_INCLUDE:FILEPATH=${STRUO_PREFIX_MAP}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" "-S" "${STRUO_SOURCE_DIR}" "-B" "${STRUO_BUILD_DIR}" ${_struo_prefix_map_args}@STRUO_CONFIGURE_ARGS@
  RESULT_VARIABLE _struo_configure_result
  COMMAND_ECHO STDOUT
)
if(NOT _struo_configure_result EQUAL 0)
  message(FATAL_ERROR "CMake configuration failed with exit code ${_struo_configure_result}")
endif()

if(DEFINED STRUO_PREFIX_MAP AND EXISTS "${STRUO_BUILD_DIR}/StruoPrefixMapEnvironment.cmake")
  include("${STRUO_BUILD_DIR}/StruoPrefixMapEnvironment.cmake")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" "--build" "${STRUO_BUILD_DIR}"@STRUO_BUILD_ARGS@
  RESULT_VARIABLE _struo_build_result
  COMMAND_ECHO STDOUT
)
if(NOT _struo_build_result EQUAL 0)
  message(FATAL_ERROR "CMake build failed with exit code ${_struo_build_result}")
endif()
"""

def _cmake_attr(ctx, key, default):
    return _configured_attr(ctx, key, default)

def _cmake_package_path(ctx, path):
    if not path or path == ".":
        return ctx["label"]["package"] or "."
    return _package_relative(ctx, path)

def _cmake_path_attrs(ctx, key):
    return [_package_relative(ctx, path) for path in _cmake_attr(ctx, key, [])]

def _cmake_globs(patterns):
    return glob(patterns) if patterns else []

def _cmake_collect(deps, key, own):
    values = []
    values.extend(own)
    for dep in deps:
        values.extend(_transitive_list(dep.get(key)))
    return _unique(values)

# Arguments are rendered as quoted CMake arguments because passing them through
# a CMake list loses boundaries at `;`, doubles `\`, and merges at unbalanced `[`.
def _cmake_quoted_args(values):
    return "".join(["\n    \"" + value.replace("\\", "\\\\").replace("\"", "\\\"").replace("$", "\\$") + "\"" for value in values])

# Copy each declared product to the products directory, following symbolic
# links. A versioned shared library is usually a link chain, and an output
# that is a link is recorded by its target text, which says nothing about the
# library, so nothing reading it could be reused.
_CMAKE_PRODUCT_COPY = """
foreach(_struo_product IN ITEMS@STRUO_PRODUCTS@)
  get_filename_component(_struo_real "${STRUO_BUILD_DIR}/${_struo_product}" REALPATH)
  set(_struo_copy "${STRUO_PRODUCTS_DIR}/${_struo_product}")
  if(IS_DIRECTORY "${_struo_real}")
    set(_struo_mode copy_directory)
  elseif(EXISTS "${_struo_real}")
    set(_struo_mode copy)
  else()
    message(FATAL_ERROR "CMake build did not produce declared product `${_struo_product}`")
  endif()
  get_filename_component(_struo_parent "${_struo_copy}" DIRECTORY)
  file(MAKE_DIRECTORY "${_struo_parent}")
  execute_process(COMMAND "${CMAKE_COMMAND}" -E ${_struo_mode} "${_struo_real}" "${_struo_copy}" RESULT_VARIABLE _struo_copy_result)
  if(NOT _struo_copy_result EQUAL 0)
    message(FATAL_ERROR "Copying CMake product `${_struo_product}` failed")
  endif()
endforeach()
"""

# Included after the first project() call so GCC and Clang record source and
# build paths relative to the workspace, and build-tree executables find
# shared libraries relative to themselves. Debug information, __FILE__, and
# the build RUNPATH would otherwise embed the checkout path, and products
# built in two checkouts could never be reused across them. Compilers that
# predate -ffile-prefix-map, and clang-cl, keep their native flags. The
# conditions are generator expressions so languages enabled after the first
# project() are covered too.
#
# Assembly sources get -fdebug-prefix-map, but only when CMake assembles them through the C or C++
# compiler driver; a bare assembler such as GNU as rejects it. Sub-builds that
# configure separately, such as ExternalProject, see the mapping through
# CFLAGS and CXXFLAGS, but only when the action declares those variables,
# because setting one that was unset replaces a build system's own defaults.
_CMAKE_PREFIX_MAP = """
include_guard(GLOBAL)
set(_struo_prefixes "${STRUO_WORKSPACE_DIR}")
file(REAL_PATH "${STRUO_WORKSPACE_DIR}" _struo_real_workspace)
if(NOT _struo_real_workspace STREQUAL STRUO_WORKSPACE_DIR)
  list(APPEND _struo_prefixes "${_struo_real_workspace}")
endif()
foreach(_struo_language IN ITEMS C CXX OBJC OBJCXX)
  set(_struo_version "$<${_struo_language}_COMPILER_VERSION>")
  set(_struo_supported "$<OR:$<COMPILE_LANG_AND_ID:${_struo_language},IntelLLVM>,$<AND:$<COMPILE_LANG_AND_ID:${_struo_language},GNU>,$<VERSION_GREATER_EQUAL:${_struo_version},8>>,$<AND:$<COMPILE_LANG_AND_ID:${_struo_language},Clang>,$<VERSION_GREATER_EQUAL:${_struo_version},10>>,$<AND:$<COMPILE_LANG_AND_ID:${_struo_language},AppleClang>,$<VERSION_GREATER_EQUAL:${_struo_version},12>>>")
  if(NOT CMAKE_VERSION VERSION_LESS 3.30)
    set(_struo_supported "$<AND:${_struo_supported},$<NOT:$<STREQUAL:$<${_struo_language}_COMPILER_FRONTEND_VARIANT>,MSVC>>>")
  elseif(CMAKE_${_struo_language}_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC" OR (CMAKE_HOST_WIN32 AND NOT DEFINED CMAKE_${_struo_language}_COMPILER_FRONTEND_VARIANT))
    set(_struo_supported 0)
  endif()
  foreach(_struo_prefix IN LISTS _struo_prefixes)
    string(REPLACE ">" "$<ANGLE-R>" _struo_prefix "${_struo_prefix}")
    add_compile_options("$<${_struo_supported}:-ffile-prefix-map=${_struo_prefix}=.>")
  endforeach()
endforeach()
if(NOT DEFINED CMAKE_BUILD_RPATH_USE_ORIGIN)
  set(CMAKE_BUILD_RPATH_USE_ORIGIN ON)
endif()
# Whether assembly can be mapped is known only once every language is
# enabled, so the options live on a property filled in later and read when
# the build is generated, which also covers targets created after that.
# Imported, so it is not among the project's own targets and cannot be
# installed or exported.
add_library(_struo_prefix_map INTERFACE IMPORTED GLOBAL)
add_compile_options("$<$<COMPILE_LANGUAGE:ASM>:$<TARGET_PROPERTY:_struo_prefix_map,STRUO_ASM_OPTIONS>>")

function(_struo_supported_driver language out)
  set(_struo_id "${CMAKE_${language}_COMPILER_ID}")
  set(_struo_version "${CMAKE_${language}_COMPILER_VERSION}")
  get_property(_struo_languages GLOBAL PROPERTY ENABLED_LANGUAGES)
  list(FIND _struo_languages "${language}" _struo_index)
  set(${out} FALSE PARENT_SCOPE)
  if(_struo_index EQUAL -1 OR CMAKE_${language}_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC")
    return()
  endif()
  if(_struo_id STREQUAL "IntelLLVM"
      OR (_struo_id STREQUAL "GNU" AND _struo_version VERSION_GREATER_EQUAL 8)
      OR (_struo_id STREQUAL "Clang" AND _struo_version VERSION_GREATER_EQUAL 10)
      OR (_struo_id STREQUAL "AppleClang" AND _struo_version VERSION_GREATER_EQUAL 12))
    set(${out} TRUE PARENT_SCOPE)
  endif()
endfunction()

function(_struo_finish_prefix_map)
  set(_struo_prefixes "${STRUO_WORKSPACE_DIR}")
  file(REAL_PATH "${STRUO_WORKSPACE_DIR}" _struo_real_workspace)
  if(NOT _struo_real_workspace STREQUAL STRUO_WORKSPACE_DIR)
    list(APPEND _struo_prefixes "${_struo_real_workspace}")
  endif()

  set(_struo_driver FALSE)
  foreach(_struo_language IN ITEMS C CXX)
    _struo_supported_driver(${_struo_language} _struo_supported)
    if(_struo_supported AND DEFINED CMAKE_ASM_COMPILER AND CMAKE_ASM_COMPILER STREQUAL CMAKE_${_struo_language}_COMPILER)
      set(_struo_driver TRUE)
    endif()
  endforeach()
  if(_struo_driver)
    set(_struo_options)
    foreach(_struo_prefix IN LISTS _struo_prefixes)
      list(APPEND _struo_options "-fdebug-prefix-map=${_struo_prefix}=.")
    endforeach()
    set_property(TARGET _struo_prefix_map PROPERTY STRUO_ASM_OPTIONS "${_struo_options}")
  endif()

  set(_struo_environment "")
  foreach(_struo_pair IN ITEMS "C=CFLAGS" "CXX=CXXFLAGS")
    string(REPLACE "=" ";" _struo_pair "${_struo_pair}")
    list(GET _struo_pair 0 _struo_language)
    list(GET _struo_pair 1 _struo_variable)
    _struo_supported_driver(${_struo_language} _struo_supported)
    if(NOT _struo_supported)
      continue()
    endif()
    set(_struo_flags "")
    foreach(_struo_prefix IN LISTS _struo_prefixes)
      # Flags in the environment are split on whitespace, so a path that
      # contains any cannot be passed this way.
      string(FIND "${_struo_prefix}" "\\\\" _struo_backslash)
      if(_struo_backslash EQUAL -1 AND NOT _struo_prefix MATCHES "[ \\t\\n\\"]")
        string(APPEND _struo_flags " -ffile-prefix-map=${_struo_prefix}=.")
      endif()
    endforeach()
    if(_struo_flags)
      string(APPEND _struo_environment "if(DEFINED ENV{${_struo_variable}})\\n  set(ENV{${_struo_variable}} \\"\\$ENV{${_struo_variable}}${_struo_flags}\\")\\nendif()\\n")
    endif()
  endforeach()
  file(WRITE "${CMAKE_BINARY_DIR}/StruoPrefixMapEnvironment.cmake" "${_struo_environment}")
endfunction()
cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL _struo_finish_prefix_map)
"""

# An exported build tree is shared by the targets that run in or copy from it,
# and each test batch runs in its own Struo process that builds the tree
# again, so the build runs under a lock and is skipped when nothing it reads
# changed since the last build, which leaves the tree untouched while another
# process runs tests in it. Declared inputs are compared by content and
# everything else by path, size, and modification time and a Ninja dry run. The tree starts again from scratch when the toolchain identity,
# configure arguments, or the bytes of a compiler CMake selected differ from
# the ones it was built with, because CMake does not notice those itself.
_CMAKE_SHARED_TREE_BEGIN = """
cmake_minimum_required(VERSION 3.20)
file(LOCK "${STRUO_BUILD_DIR}.lock" GUARD PROCESS TIMEOUT 7200)
function(_struo_tree_identity out)
  set(_struo_identity "${STRUO_TREE_IDENTITY}")
  if(EXISTS "${STRUO_BUILD_DIR}/CMakeCache.txt")
    file(STRINGS "${STRUO_BUILD_DIR}/CMakeCache.txt" _struo_compilers REGEX "^CMAKE_[A-Za-z0-9_]*_COMPILER:[A-Z]+=")
    foreach(_struo_line IN LISTS _struo_compilers)
      string(REGEX REPLACE "^[^=]*=" "" _struo_path "${_struo_line}")
      if(EXISTS "${_struo_path}" AND NOT IS_DIRECTORY "${_struo_path}")
        file(SHA256 "${_struo_path}" _struo_digest)
        string(APPEND _struo_identity "\\n${_struo_path}=${_struo_digest}")
      endif()
    endforeach()
  endif()
  set(${out} "${_struo_identity}" PARENT_SCOPE)
endfunction()
set(_struo_stamp "${STRUO_BUILD_DIR}/struo-tree-identity")
set(_struo_fresh TRUE)
if(EXISTS "${_struo_stamp}")
  file(READ "${_struo_stamp}" _struo_recorded)
  _struo_tree_identity(_struo_current)
  if(_struo_recorded STREQUAL _struo_current)
    set(_struo_fresh FALSE)
  endif()
endif()
if(_struo_fresh)
  file(REMOVE_RECURSE "${STRUO_BUILD_DIR}")
endif()
set(STRUO_TREE_TARGETS@STRUO_TREE_TARGETS@)
set(_struo_state "")
file(STRINGS "${STRUO_INPUTS_LIST}" _struo_inputs)
foreach(_struo_input IN LISTS _struo_inputs)
  if(EXISTS "${STRUO_ROOT}/${_struo_input}" AND NOT IS_DIRECTORY "${STRUO_ROOT}/${_struo_input}")
    file(SHA256 "${STRUO_ROOT}/${_struo_input}" _struo_digest)
    string(APPEND _struo_state "${_struo_input}=${_struo_digest}\\n")
  endif()
endforeach()
string(SHA256 _struo_state "${_struo_state}")

# Empty when Ninja has nothing to do, which also covers files outside the
# source tree such as system headers and libraries the last build read;
# otherwise a digest of the commands it would run, or `unknown`.
function(_struo_pending_work out)
  set(${out} "unknown" PARENT_SCOPE)
  if(NOT EXISTS "${STRUO_BUILD_DIR}/CMakeCache.txt")
    return()
  endif()
  file(STRINGS "${STRUO_BUILD_DIR}/CMakeCache.txt" _struo_program REGEX "^CMAKE_MAKE_PROGRAM:[A-Z]+=")
  string(REGEX REPLACE "^[^=]*=" "" _struo_program "${_struo_program}")
  if(NOT _struo_program)
    return()
  endif()
  execute_process(COMMAND "${_struo_program}" -C "${STRUO_BUILD_DIR}" -n ${STRUO_TREE_TARGETS}
    RESULT_VARIABLE _struo_result OUTPUT_VARIABLE _struo_output ERROR_QUIET)
  if(NOT _struo_result EQUAL 0)
    return()
  endif()
  if(_struo_output MATCHES "ninja: no work to do")
    set(${out} "" PARENT_SCOPE)
    return()
  endif()
  string(REGEX REPLACE "\\[[0-9]+/[0-9]+\\] " "" _struo_output "${_struo_output}")
  string(SHA256 _struo_output "${_struo_output}")
  set(${out} "${_struo_output}" PARENT_SCOPE)
endfunction()

# Every file in the source tree by path, size, and modification time. Ninja
# does not see files only configure reads, such as one read with file(READ),
# or ones a custom target that always runs copies without listing them.
function(_struo_walk_state out)
  set(_struo_walk "")
  file(GLOB _struo_entries LIST_DIRECTORIES true "${STRUO_SOURCE_DIR}/*" "${STRUO_SOURCE_DIR}/.*")
  list(SORT _struo_entries)
  foreach(_struo_entry IN LISTS _struo_entries)
    get_filename_component(_struo_name "${_struo_entry}" NAME)
    if(_struo_name STREQUAL "." OR _struo_name STREQUAL ".." OR _struo_name STREQUAL ".git" OR _struo_name STREQUAL ".struo" OR EXISTS "${_struo_entry}/CMakeCache.txt")
      continue()
    endif()
    if(IS_DIRECTORY "${_struo_entry}")
      file(GLOB_RECURSE _struo_files LIST_DIRECTORIES false "${_struo_entry}/*")
      list(SORT _struo_files)
    else()
      set(_struo_files "${_struo_entry}")
    endif()
    foreach(_struo_file IN LISTS _struo_files)
      if(NOT EXISTS "${_struo_file}" OR IS_DIRECTORY "${_struo_file}")
        continue()
      endif()
      file(TIMESTAMP "${_struo_file}" _struo_time "%s" UTC)
      file(SIZE "${_struo_file}" _struo_size)
      file(RELATIVE_PATH _struo_relative "${STRUO_SOURCE_DIR}" "${_struo_file}")
      string(APPEND _struo_walk "${_struo_relative} ${_struo_size} ${_struo_time}\\n")
    endforeach()
  endforeach()
  string(SHA256 _struo_walk "${_struo_walk}")
  set(${out} "${_struo_walk}" PARENT_SCOPE)
endfunction()

set(_struo_state_stamp "${STRUO_BUILD_DIR}/struo-tree-sources")
set(_struo_work_stamp "${STRUO_BUILD_DIR}/struo-tree-work")
set(_struo_walk_stamp "${STRUO_BUILD_DIR}/struo-tree-walk")
set(_struo_skip FALSE)
if(NOT _struo_fresh AND EXISTS "${_struo_state_stamp}" AND EXISTS "${_struo_work_stamp}" AND EXISTS "${_struo_walk_stamp}")
  file(READ "${_struo_state_stamp}" _struo_recorded_state)
  file(READ "${_struo_walk_stamp}" _struo_recorded_walk)
  _struo_walk_state(_struo_walk)
  if(_struo_recorded_state STREQUAL _struo_state AND _struo_walk STREQUAL _struo_recorded_walk)
    _struo_pending_work(_struo_work)
    file(READ "${_struo_work_stamp}" _struo_recorded_work)
    if(_struo_work STREQUAL "" OR (NOT _struo_work STREQUAL "unknown" AND _struo_work STREQUAL _struo_recorded_work))
      set(_struo_skip TRUE)
    endif()
  endif()
endif()
if(NOT _struo_skip)
  file(REMOVE "${_struo_state_stamp}" "${_struo_work_stamp}" "${_struo_walk_stamp}")
"""

_CMAKE_SHARED_TREE_END = """
  _struo_pending_work(_struo_work)
  _struo_walk_state(_struo_walk)
  file(WRITE "${_struo_work_stamp}" "${_struo_work}")
  file(WRITE "${_struo_walk_stamp}" "${_struo_walk}")
endif()
_struo_tree_identity(_struo_current)
file(WRITE "${_struo_stamp}" "${_struo_current}")
file(WRITE "${_struo_state_stamp}" "${_struo_state}")
file(READ "${_struo_work_stamp}" _struo_work)
file(READ "${_struo_walk_stamp}" _struo_walk)
string(SHA256 _struo_marker "${_struo_current}\\n${_struo_state}\\n${_struo_work}\\n${_struo_walk}")
file(WRITE "${STRUO_TREE_MARKER}" "${_struo_marker}\\n")
"""

# Copies products out of a build tree another cmake_project exported, under
# that tree's lock so a concurrent build in another process is not read
# half-done.
_CMAKE_SHARED_TREE_COPY = """
cmake_minimum_required(VERSION 3.20)
file(LOCK "${STRUO_BUILD_DIR}.lock" GUARD PROCESS TIMEOUT 7200)
"""

def _cmake_build_driver(generator, build_type, configure_args, build_targets, build_args):
    configure = (["-G", generator] if generator else []) + (["-DCMAKE_BUILD_TYPE=" + build_type] if build_type else []) + configure_args
    build = (["--config", build_type] if build_type else []) + (["--target"] + build_targets if build_targets else []) + build_args
    return _CMAKE_BUILD_DRIVER.replace("@STRUO_CONFIGURE_ARGS@", _cmake_quoted_args(configure)).replace("@STRUO_BUILD_ARGS@", _cmake_quoted_args(build))

def _cmake_product_tokens():
    os = host_os()
    return {
        "{static_prefix}": "" if os == "windows" else "lib",
        "{static_suffix}": ".lib" if os == "windows" else ".a",
        "{shared_prefix}": "" if os == "windows" else "lib",
        "{shared_suffix}": ".dll" if os == "windows" else (".dylib" if os == "macos" else ".so"),
        "{exe_suffix}": ".exe" if os == "windows" else "",
    }

def _cmake_product_path(ctx, product, attribute = "products"):
    path = product.replace("\\", "/")
    if not path or path.startswith("/") or (len(path) > 1 and path[1] == ":"):
        fail(ctx["label"]["id"] + ": " + attribute + " paths must be non-empty and relative to the generated build directory")
    components = []
    for component in path.split("/"):
        if component == "..":
            fail(ctx["label"]["id"] + ": " + attribute + " path `" + product + "` must stay inside the generated build directory")
        if component and component != ".":
            components.append(component)
    path = "/".join(components)
    for token, value in _cmake_product_tokens().items():
        path = path.replace(token, value)
    return path

_CMAKE_HEADER_EXTENSIONS = [".h", ".hh", ".hpp", ".hxx", ".inc", ".inl", ".ipp", ".tpp", ".def"]

def _cmake_static_library(path):
    return _ends_with(path, ".a") or _ends_with(path, ".lib")

def _cmake_dynamic_library(path):
    return _ends_with(path, ".so") or _ends_with(path, ".dylib") or _ends_with(path, ".dll")

def _cmake_project_inputs(ctx, headers):
    inputs = glob(ctx["srcs"])
    inputs.extend(headers)
    inputs.extend(_cmake_globs(_cmake_attr(ctx, "data", [])))
    for dep in ctx["deps"]:
        inputs.extend(_transitive_list(dep.get("transitive_headers")))
        inputs.extend(_transitive_list(dep.get("transitive_static_libraries")))
        inputs.extend(_transitive_list(dep.get("transitive_dynamic_libraries")))
        inputs.extend(_transitive_list(dep.get("transitive_data")))
    return _unique(inputs)

def _cmake_build_program(ctx, generator):
    requested = _cmake_attr(ctx, "build_program", "")
    if not requested and generator == "Ninja":
        requested = "ninja"
    if not requested:
        return ("", "")
    program = _resolve_host_executable(requested)
    if not program:
        fail(ctx["label"]["id"] + ": generated build program `" + requested + "` was not found")
    return (program, host_command([program, "--version"]).strip())

def _cmake_project_impl(ctx):
    products = [_cmake_product_path(ctx, product) for product in _cmake_attr(ctx, "products", [])]
    if "" in products:
        fail(ctx["label"]["id"] + ": products must name files or directories inside the generated build directory, not the directory itself")
    export_tree = _cmake_attr(ctx, "export_build_tree", False)
    if not products and not export_tree:
        fail(ctx["label"]["id"] + ": products must list at least one file produced under the generated CMake build directory")

    cmake = _resolve_host_executable(_cmake_attr(ctx, "cmake", "cmake"))
    if not cmake:
        fail(ctx["label"]["id"] + ": CMake executable was not found")
    version = host_command([cmake, "--version"]).strip()
    source_dir = _cmake_package_path(ctx, _cmake_attr(ctx, "source_dir", "."))
    build_root = ctx["scratch_dir"] + "/cmake-build"
    driver = ctx["scratch_dir"] + "/StruoCMakeBuild.cmake"
    generator = _cmake_attr(ctx, "generator", "Ninja")
    build_type = _cmake_attr(ctx, "build_type", "Debug")
    build_program, build_program_version = _cmake_build_program(ctx, generator)
    # A tree CTest runs in place keeps native paths, because tests may locate
    # their data through __FILE__.
    prefix_map = "" if _cmake_attr(ctx, "native_paths", False) else ctx["scratch_dir"] + "/StruoPrefixMap.cmake"
    if prefix_map:
        write_path(prefix_map, _CMAKE_PREFIX_MAP)
    driver_files = [driver] + ([prefix_map] if prefix_map else [])
    configure_args = _cmake_attr(ctx, "configure_args", [])
    if build_program:
        configure_args = ["-DCMAKE_MAKE_PROGRAM:FILEPATH=" + build_program] + configure_args
    products_dir = ctx["scratch_dir"] + "/cmake-products"
    raw_products = [products_dir + "/" + product for product in products]
    tree_marker = products_dir + "/struo-build-tree" if export_tree else ""
    headers = _unique(_cmake_path_attrs(ctx, "hdrs") + _cmake_globs(_cmake_attr(ctx, "header_globs", [])))
    inputs = _cmake_project_inputs(ctx, headers)

    action_env = dict(_cmake_attr(ctx, "env", {}))
    if not action_env.get("PATH"):
        action_env["PATH"] = host_env("PATH")
    shared = [dep for dep in ctx["deps"] if dep.get("cmake_build_tree")]
    if shared and not export_tree:
        if len(shared) > 1:
            fail(ctx["label"]["id"] + ": cmake_project can copy from one exported build tree, got " + str(len(shared)))
        write_path(driver, _CMAKE_SHARED_TREE_COPY + _CMAKE_PRODUCT_COPY.replace("@STRUO_PRODUCTS@", _cmake_quoted_args(products)))
        build_root = shared[0]["cmake_build_tree"]
        run_action(
            display_name = "Copy CMake products · " + ctx["label"]["name"],
            history = _action_history(ctx, "cmake", "copy-products"),
            source_files = _action_source_files(_unique(inputs + [driver])),
            argv = [
                cmake,
                "-DSTRUO_BUILD_DIR:PATH=" + execution_path(build_root),
                "-DSTRUO_PRODUCTS_DIR:PATH=" + execution_path(products_dir),
                "-P", execution_path(driver),
            ],
            inputs = _unique([shared[0]["cmake_build_tree_marker"], driver] + inputs),
            outputs = raw_products,
            clean_paths = [products_dir],
            env = action_env,
            toolchain_identity = "struo.cmake.project-copy.v1\x00" + version,
            cacheable = False,
            identifier = ctx["label"]["id"] + ":cmake-build",
        )
        return _cmake_project_provider(ctx, products, raw_products, headers, inputs, export_tree, build_root, tree_marker)

    build_targets = _cmake_attr(ctx, "build_targets", [])
    build_driver = _cmake_build_driver(generator, build_type, configure_args, build_targets, _cmake_attr(ctx, "build_args", []))
    inputs_list = ctx["scratch_dir"] + "/StruoTreeInputs.txt"
    if export_tree:
        build_driver = _CMAKE_SHARED_TREE_BEGIN.replace("@STRUO_TREE_TARGETS@", _cmake_quoted_args(build_targets)) + build_driver + _CMAKE_SHARED_TREE_END
        write_path(inputs_list, "".join([path + "\n" for path in sorted(inputs)]))
        driver_files = driver_files + [inputs_list]
    write_path(driver, build_driver + _CMAKE_PRODUCT_COPY.replace("@STRUO_PRODUCTS@", _cmake_quoted_args(products)))
    toolchain_identity = "struo.cmake.project.v1\x00" + version + "\x00" + generator + "\x00" + build_program + "\x00" + build_program_version + "\x00" + build_type
    run_action(
        display_name = "Configure and build CMake project · " + ctx["label"]["name"],
        history = _action_history(ctx, "cmake", "configure-and-build"),
        presentation = _action_metadata(context = [_action_context("cmake.generator", generator), _action_context("cmake.build_type", build_type)]),
        source_files = _action_source_files(_unique(inputs + driver_files)),
        argv = [
            cmake,
            "-DSTRUO_SOURCE_DIR:PATH=" + execution_path(source_dir),
            "-DSTRUO_BUILD_DIR:PATH=" + execution_path(build_root),
            "-DSTRUO_PRODUCTS_DIR:PATH=" + execution_path(products_dir),
        ] + ([
            "-DSTRUO_WORKSPACE_DIR:PATH=" + execution_path(""),
            "-DSTRUO_PREFIX_MAP:FILEPATH=" + execution_path(prefix_map),
        ] if prefix_map else []) + ([
            "-DSTRUO_TREE_IDENTITY:STRING=" + content_sha256(toolchain_identity + "\x00" + _json_encode(configure_args) + "\x00" + _json_encode(action_env)),
            "-DSTRUO_TREE_MARKER:FILEPATH=" + execution_path(tree_marker),
            "-DSTRUO_ROOT:PATH=" + execution_path(""),
            "-DSTRUO_INPUTS_LIST:FILEPATH=" + execution_path(inputs_list),
        ] if export_tree else []) + [
            "-P", execution_path(driver),
        ],
        inputs = _unique(inputs + driver_files),
        outputs = raw_products + ([tree_marker] if export_tree else []),
        clean_paths = [products_dir] if export_tree else [build_root, products_dir],
        env = action_env,
        toolchain_identity = toolchain_identity,
        cacheable = False,
        identifier = ctx["label"]["id"] + ":cmake-build",
    )
    return _cmake_project_provider(ctx, products, raw_products, headers, inputs, export_tree, build_root, tree_marker)

def _cmake_project_provider(ctx, products, raw_products, headers, inputs, export_tree, build_root, tree_marker):
    staged_products = []
    for index in range(len(products)):
        staged = declare_output("products/" + products[index])
        copy_path(
            raw_products[index],
            staged,
            inputs = [raw_products[index]],
            toolchain_identity = "struo.cmake.product.v1",
            identifier = ctx["label"]["id"] + ":cmake-product:" + products[index],
            source_files = _action_source_files([raw_products[index]]),
        )
        staged_products.append(staged)

    own_static = [path for path in staged_products if _cmake_static_library(path)]
    own_dynamic = [path for path in staged_products if _cmake_dynamic_library(path)]
    products_root = staged_products[0][:-len(products[0])].rstrip("/") if staged_products else ""
    include_dirs = _cmake_path_attrs(ctx, "includes")
    for directory in _cmake_attr(ctx, "generated_includes", []):
        relative = _cmake_product_path(ctx, directory, "generated_includes")
        generated = [
            staged_products[index]
            for index in range(len(products))
            if products[index] == relative or ((not relative or products[index].startswith(relative + "/")) and _filter_by_extensions([products[index]], _CMAKE_HEADER_EXTENSIONS))
        ]
        if not generated:
            fail(ctx["label"]["id"] + ": generated include directory `" + directory + "` contains no declared header product; list the headers CMake generates there, or the directory itself, in products")
        headers = _unique(headers + generated)
        include_dirs.append(products_root + "/" + relative if relative else products_root)
    defines = _cmake_attr(ctx, "defines", [])
    linkopts = _cmake_attr(ctx, "linkopts", [])
    data = _cmake_globs(_cmake_attr(ctx, "data", []))
    transitive_headers = _cmake_collect(ctx["deps"], "transitive_headers", headers)
    transitive_include_dirs = _cmake_collect(ctx["deps"], "transitive_include_dirs", include_dirs)
    transitive_defines = _cmake_collect(ctx["deps"], "transitive_defines", defines)
    transitive_static = _cmake_collect(ctx["deps"], "transitive_static_libraries", own_static)
    transitive_dynamic = _cmake_collect(ctx["deps"], "transitive_dynamic_libraries", own_dynamic)
    transitive_linkopts = _cmake_collect(ctx["deps"], "transitive_linkopts", linkopts)
    transitive_data = _cmake_collect(ctx["deps"], "transitive_data", data)
    archive = own_static[0] if own_static else ""
    return {
        "cmake_project": True,
        "c_provider": True,
        "label_id": ctx["label"]["id"],
        "target_kind": "cmake_project",
        "products": staged_products,
        "archive": archive,
        "static_library": archive,
        "static_libraries": own_static,
        "dynamic_libraries": own_dynamic,
        "headers": headers,
        "include_dirs": include_dirs,
        "defines": defines,
        "linkopts": linkopts,
        "transitive_headers": transitive_headers,
        "transitive_include_dirs": transitive_include_dirs,
        "transitive_quote_include_dirs": _cmake_collect(ctx["deps"], "transitive_quote_include_dirs", []),
        "transitive_system_include_dirs": _cmake_collect(ctx["deps"], "transitive_system_include_dirs", []),
        "transitive_framework_include_dirs": _cmake_collect(ctx["deps"], "transitive_framework_include_dirs", []),
        "transitive_defines": transitive_defines,
        "transitive_static_libraries": transitive_static,
        "transitive_dynamic_libraries": transitive_dynamic,
        "transitive_linkopts": transitive_linkopts,
        "transitive_data": transitive_data,
        "transitive_archives": transitive_static,
        # An exported tree serves test targets that declare their own inputs,
        # so it selects no tests itself.
        "affected_inputs": [] if export_tree else _unique(inputs + transitive_data),
        "default_output": staged_products[0] if staged_products else build_root,
        "cmake_build_tree": build_root if export_tree else "",
        "cmake_build_tree_marker": tree_marker,
    }

def _cmake_regex_literal(value):
    escaped = ""
    for character in value.elems():
        if character == "$":
            escaped += "[$]"
        else:
            escaped += ("\\" + character) if character in "\\^.|?*+()[]" else character
    return escaped

_CMAKE_TEST_PATH_GUARD = """
cmake_minimum_required(VERSION 3.20)
get_filename_component(_struo_expected "${STRUO_EXPECTED_ROOT}" REALPATH)
get_filename_component(_struo_actual "${STRUO_EXECUTION_ROOT}" REALPATH)
if(NOT _struo_expected STREQUAL _struo_actual)
  message(FATAL_ERROR "cmake_test requires execution at its original workspace path; relocated sandbox and remote execution are unsupported")
endif()
"""

def _cmake_test_impl(ctx):
    cmake = _resolve_host_executable(_cmake_attr(ctx, "cmake", "cmake"))
    test_targets = _cmake_attr(ctx, "test_targets", [])
    ctest = _resolve_host_executable(_cmake_attr(ctx, "ctest", "ctest")) if not test_targets else ""
    if not cmake or (not test_targets and not ctest):
        fail(ctx["label"]["id"] + ": cmake_test requires CMake and, unless test_targets is set, CTest executables")
    jobs = _cmake_attr(ctx, "jobs", 1)
    if jobs < 1:
        fail(ctx["label"]["id"] + ": jobs must be positive")
    timeout_ms = _cmake_attr(ctx, "timeout_ms", None)
    if timeout_ms != None and timeout_ms <= 0:
        fail(ctx["label"]["id"] + ": timeout_ms must be positive")
    shared_trees = [dep["cmake_build_tree"] for dep in ctx["deps"] if dep.get("cmake_build_tree")]
    shared_markers = [dep["cmake_build_tree_marker"] for dep in ctx["deps"] if dep.get("cmake_build_tree")]
    if len(shared_trees) > 1:
        fail(ctx["label"]["id"] + ": cmake_test can run in one exported build tree, got " + str(len(shared_trees)))
    shared_tree = shared_trees[0] if shared_trees else ""
    if shared_tree and test_targets:
        fail(ctx["label"]["id"] + ": test_targets cannot be combined with an exported build tree dependency")
    if shared_tree and _cmake_attr(ctx, "coverage_per_test", False):
        fail(ctx["label"]["id"] + ": coverage_per_test cannot be combined with an exported build tree dependency")
    tests = _cmake_attr(ctx, "tests", [])
    if tests and test_targets:
        fail(ctx["label"]["id"] + ": tests selects CTest tests and cannot be combined with test_targets")
    build_root = shared_tree or ctx["build_dir"] + "/cmake-build"
    test_dir = ctx["build_dir"] + "/test"
    results = test_dir + "/test_results.json"
    log = test_dir + ("/cmake-test.log" if test_targets else "/ctest.log")
    native_results = test_dir + "/ctest.xml"
    native_artifacts = [] if test_targets else [native_results]
    suite_name = "CMake test targets" if test_targets else "CTest suite"
    driver = ctx["scratch_dir"] + "/StruoCTest.cmake"
    action_env = dict(_cmake_attr(ctx, "env", {}))
    if not action_env.get("PATH"):
        action_env["PATH"] = host_env("PATH")
    if not action_env.get("CMAKE_BUILD_PARALLEL_LEVEL"):
        action_env["CMAKE_BUILD_PARALLEL_LEVEL"] = str(jobs)
    path_args = [
        "-DSTRUO_EXPECTED_ROOT:PATH=" + workspace_root(),
        "-DSTRUO_EXECUTION_ROOT:PATH=" + execution_path("."),
    ]
    argv = [cmake] + path_args + [
        "-DSTRUO_BUILD_DIR:PATH=" + execution_path(build_root),
        "-DSTRUO_TEST_DIR:PATH=" + execution_path(test_dir),
        "-DSTRUO_RESULTS:FILEPATH=" + execution_path(results),
        "-DSTRUO_LOG:FILEPATH=" + execution_path(log),
        "-DSTRUO_JUNIT:FILEPATH=" + execution_path(native_results),
        "-P", execution_path(driver),
    ]
    info = {
        "schema": "struo.test_info.v1",
        "target": ctx["label"]["id"],
        "runner": {"type": "cmake_test", "display_name": suite_name, "metadata": {}},
        "command": {"argv": argv, "env": action_env, "cwd": "."},
        "outputs": {"results": results, "logs": [log], "native_results": native_artifacts, "coverage": []},
        "listing": {"supported": False, "strategy": "suite"},
        "filtering": {"case_filtering": "unsupported"},
        "sharding": {"supported": False},
        "retries": {"supported": False, "default_attempts": 1},
        "execution": {"cacheable": False, "timeout_ms": timeout_ms, "run_from_workspace_root": True},
        "labels": _cmake_attr(ctx, "labels", []),
        "metadata": {"counts": "suite", "path_bound": True},
    }
    provider = {"label_id": ctx["label"]["id"], "target_kind": "cmake_test", "default_output": build_root, "test_info": info}
    coverage = _cmake_coverage_configuration(ctx, provider, cmake, ctest, action_env) if _cmake_attr(ctx, "coverage_per_test", False) else None
    if ctx["capability"] == "metadata":
        return provider

    build_type = _cmake_attr(ctx, "build_type", "Debug")
    generator = _cmake_attr(ctx, "generator", "Ninja")
    program, program_version = _cmake_build_program(ctx, generator)
    configure_args = _cmake_attr(ctx, "configure_args", [])
    if program:
        configure_args = ["-DCMAKE_MAKE_PROGRAM:FILEPATH=" + program] + configure_args
    inputs = _unique(glob(ctx["srcs"]) + _cmake_globs(_cmake_attr(ctx, "data", [])))
    identity = "struo.cmake.test.v1\x00" + host_os() + "\x00" + host_arch() + "\x00" + content_sha256(workspace_root()) + "\x00" + host_command([cmake, "--version"]) + "\x00" + program_version
    if not shared_tree:
        build_driver = ctx["scratch_dir"] + "/StruoCTestBuild.cmake"
        write_path(build_driver, _CMAKE_TEST_PATH_GUARD + _cmake_build_driver(generator, build_type, configure_args, _cmake_attr(ctx, "build_targets", []), ["--parallel", str(jobs)]))
        run_action(
            argv = [cmake] + path_args + [
                "-DSTRUO_SOURCE_DIR:PATH=" + execution_path(_cmake_package_path(ctx, _cmake_attr(ctx, "source_dir", "."))),
                "-DSTRUO_BUILD_DIR:PATH=" + execution_path(build_root),
                "-P", execution_path(build_driver),
            ],
            inputs = inputs + [build_driver],
            outputs = [build_root],
            clean_paths = [build_root],
            env = action_env,
            toolchain_identity = identity,
            cacheable = False,
            resources = {"cpu_slots": jobs, "memory_bytes": jobs * 1024 * 1024 * 1024},
            identifier = ctx["label"]["id"] + ":cmake-test-build",
            display_name = "Build CMake test suite · " + ctx["label"]["name"],
        )
    if coverage and ctx["capability"] in ["test", "test-discovery"]:
        _cmake_coverage_actions(ctx, provider, coverage, ctest, action_env, inputs, build_root, build_type)
        return provider
    config = ["--build-config", build_type] if build_type else []
    test_argv = [ctest, "--test-dir", "${STRUO_BUILD_DIR}"] + config + ["--parallel", str(jobs), "--output-on-failure", "--no-tests=error", "--output-junit", "${STRUO_JUNIT}"]
    if tests:
        test_argv += ["--tests-regex", "^(" + "|".join([_cmake_regex_literal(name) for name in tests]) + ")$"]
    if test_targets:
        test_argv = [cmake, "--build", "${STRUO_BUILD_DIR}"] + (["--config", build_type] if build_type else []) + ["--parallel", str(jobs), "--target"] + test_targets
    command = " ".join(['"' + arg.replace("\\", "\\\\").replace('"', '\\"') + '"' for arg in test_argv])
    normalized = '{"schema":"struo.test_results.v1","target":' + _json_string(ctx["label"]["id"]) + ',"runner":{"type":"cmake_test","metadata":{}},"status":"@status@","summary":{"total":1,"passed":@passed@,"failed":@failed@,"skipped":0,"flaky":0},"cases":[{"id":' + _json_string(ctx["label"]["id"] + "::suite") + ',"name":' + _json_string(ctx["label"]["name"]) + ',"suite":' + _json_string(ctx["label"]["id"]) + ',"status":"@status@","attempts":[{"status":"@status@"}],"runner_metadata":{}}],"artifacts":{"logs":[' + _json_string(log) + '],"native_results":[' + ",".join([_json_string(path) for path in native_artifacts]) + ']}}'
    timeout = " TIMEOUT " + str(timeout_ms) + "e-3" if timeout_ms != None else ""
    write_path(driver, _CMAKE_TEST_PATH_GUARD + """
file(MAKE_DIRECTORY "${STRUO_TEST_DIR}")
execute_process(COMMAND """ + command + """
  RESULT_VARIABLE code OUTPUT_FILE "${STRUO_LOG}" ERROR_FILE "${STRUO_LOG}"
  """ + timeout + """
)
if(code EQUAL 0)
  set(status passed)
  set(passed 1)
  set(failed 0)
else()
  set(status failed)
  set(passed 0)
  set(failed 1)
  file(APPEND "${STRUO_LOG}" "\\nCMake test runner failed: ${code}\\n")
endif()
set(normalized [====[""" + normalized + """ ]====])
string(CONFIGURE "${normalized}" normalized @ONLY)
file(WRITE "${STRUO_RESULTS}" "${normalized}\\n")
file(READ "${STRUO_LOG}" output LIMIT 65536)
message("${output}")
file(SIZE "${STRUO_LOG}" output_size)
if(output_size GREATER 65536)
  message("CMake test console output truncated; full output is retained in ${STRUO_LOG}")
endif()
if(NOT code EQUAL 0)
  message(FATAL_ERROR "CMake test runner failed: ${code}")
endif()
""")
    if ctx["capability"] == "test":
        run_action(
            argv = argv,
            inputs = _unique((shared_markers if shared_tree else [build_root]) + [driver] + inputs),
            outputs = [test_dir],
            clean_paths = [test_dir],
            env = action_env,
            toolchain_identity = identity + "\x00" + (host_command([ctest, "--version"]) if ctest else "cmake-test-targets"),
            cacheable = False,
            resources = {"cpu_slots": jobs, "memory_bytes": jobs * 1024 * 1024 * 1024},
            identifier = ctx["label"]["id"] + ":ctest-run",
            display_name = "Run " + suite_name + " · " + ctx["label"]["name"],
        )
    return provider

def _cmake_resolver_file(ctx, path, description):
    key = path[2:] if path.startswith("./") else path
    content = (ctx.get("files") or {}).get(key)
    if content == None:
        fail(ctx["label"]["id"] + ": " + description + " `" + path + "` must be included in resolver_inputs, or srcs when resolver_inputs is omitted")
    return content

def _cmake_snapshot_inputs(ctx):
    attrs = ctx.get("attrs") or ctx.get("attr") or {}
    return _resolver_snapshot_inputs(ctx.get("files") or {}, [attrs.get("snapshot") or ""])

def _cmake_snapshot_selection(ctx):
    attrs = ctx.get("attrs") or ctx.get("attr") or {}
    return {
        "source_dir": attrs.get("source_dir") or ".",
        "generator": attrs.get("generator") or "Ninja",
        "build_type": attrs.get("build_type") or "Debug",
    }

def _cmake_validate_snapshot(ctx, snapshot, path):
    if snapshot.get("schema") != "struo.cmake.snapshot.v1":
        fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` must use schema `struo.cmake.snapshot.v1`")
    binding = snapshot.get("struo_snapshot")
    if type(binding) != type({}):
        fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` has no struo_snapshot provenance")
    expected_inputs = _cmake_snapshot_inputs(ctx)
    bound_inputs = binding.get("inputs")
    if type(bound_inputs) != type({}) or sorted(bound_inputs.keys()) != sorted(expected_inputs.keys()):
        fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` input set does not match resolver_inputs")
    for input_path, content in expected_inputs.items():
        if bound_inputs.get(input_path) != content:
            fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` is stale relative to `" + input_path + "`")
    selection = binding.get("selection")
    if type(selection) != type({}):
        fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` has no configuration selection provenance")
    for key, value in _cmake_snapshot_selection(ctx).items():
        if selection.get(key) != value:
            fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` selection `" + key + "` does not match the target")
    if not binding.get("fingerprint"):
        fail(ctx["label"]["id"] + ": CMake snapshot `" + path + "` has no normalized fingerprint")

def _cmake_snapshot_targets(snapshot):
    targets = snapshot.get("targets")
    if type(targets) != type([]):
        fail("CMake snapshot targets must be a list")
    by_name = {}
    for target in targets:
        if type(target) != type({}):
            fail("CMake snapshot targets must contain only records")
        name = target.get("struo_name")
        if not name:
            fail("CMake snapshot target has no struo_name")
        if name in by_name:
            fail("CMake snapshot contains duplicate target `" + name + "`")
        by_name[name] = target
    for target in targets:
        for dep in target.get("deps") or []:
            if dep not in by_name:
                fail("CMake snapshot target `" + target["struo_name"] + "` references missing dependency `" + dep + "`")
    return (targets, by_name)

def _cmake_selected_target_names(ctx, snapshot, targets, by_name):
    requested = (ctx.get("attrs") or {}).get("exports") or snapshot.get("exports") or []
    selected = {}
    for value in requested:
        match = value if value in by_name else ""
        if not match:
            for target in targets:
                if target.get("name") == value:
                    match = target["struo_name"]
                    break
        if not match:
            fail(ctx["label"]["id"] + ": requested CMake export `" + value + "` is absent from the snapshot")
        selected[match] = True
    if not selected:
        for target in targets:
            selected[target["struo_name"]] = True
    for _ in range(len(targets) + 1):
        for target in targets:
            if selected.get(target["struo_name"]):
                for dep in target.get("deps") or []:
                    selected[dep] = True
    return selected

def _cmake_target_spec(target):
    return {
        "name": target["struo_name"],
        "kind": "cmake_target",
        "deps": target.get("deps") or [],
        "srcs": target.get("sources") or [],
        "attrs": {
            "cmake_name": target.get("name") or target["struo_name"],
            "cmake_type": target.get("type") or "",
            "artifacts": target.get("artifacts") or [],
            "include_dirs": target.get("include_dirs") or [],
            "compile_definitions": target.get("compile_definitions") or [],
            "snapshot_fingerprint": target.get("snapshot_fingerprint") or "",
            "_cmake_resolved": True,
        },
    }

def _cmake_workspace_resolver(ctx):
    attrs = ctx.get("attrs") or {}
    path = attrs.get("snapshot") or ""
    if not path:
        fail(ctx["label"]["id"] + ": snapshot is required")
    snapshot = json_decode(_cmake_resolver_file(ctx, path, "CMake snapshot"))
    _cmake_validate_snapshot(ctx, snapshot, path)
    targets, by_name = _cmake_snapshot_targets(snapshot)
    selected = _cmake_selected_target_names(ctx, snapshot, targets, by_name)
    specs = [_cmake_target_spec(target) for target in targets if selected.get(target["struo_name"])]
    roots = []
    requested = attrs.get("exports") or snapshot.get("exports") or []
    if requested:
        for value in requested:
            if value in by_name:
                roots.append(value)
            else:
                for target in targets:
                    if target.get("name") == value:
                        roots.append(target["struo_name"])
                        break
    else:
        roots = [spec["name"] for spec in specs]
    binding = snapshot["struo_snapshot"]
    return {
        "targets": specs,
        "roots": _unique(roots),
        "attrs": {
            "_cmake_resolved": True,
            "_cmake_snapshot_fingerprint": binding["fingerprint"],
            "_cmake_exports": _unique(roots),
        },
    }

def _cmake_workspace_impl(ctx):
    if not _cmake_attr(ctx, "_cmake_resolved", False):
        fail(ctx["label"]["id"] + ": cmake_workspace must be expanded by its graph resolver before analysis")
    return {
        "cmake_workspace": True,
        "label_id": ctx["label"]["id"],
        "snapshot_fingerprint": _cmake_attr(ctx, "_cmake_snapshot_fingerprint", ""),
        "exports": _cmake_attr(ctx, "_cmake_exports", []),
        "targets": ctx["deps"],
    }

def _cmake_target_impl(ctx):
    if not _cmake_attr(ctx, "_cmake_resolved", False):
        fail(ctx["label"]["id"] + ": cmake_target is resolver-owned and must come from cmake_workspace")
    return {
        "cmake_target": True,
        "label_id": ctx["label"]["id"],
        "cmake_name": _cmake_attr(ctx, "cmake_name", ctx["label"]["name"]),
        "cmake_type": _cmake_attr(ctx, "cmake_type", ""),
        "artifacts": _cmake_attr(ctx, "artifacts", []),
        "sources": glob(ctx["srcs"]),
        "include_dirs": _cmake_attr(ctx, "include_dirs", []),
        "compile_definitions": _cmake_attr(ctx, "compile_definitions", []),
        "snapshot_fingerprint": _cmake_attr(ctx, "snapshot_fingerprint", ""),
        "dependencies": [dep.get("label_id") for dep in ctx["deps"]],
    }

# Native discovery configures the project once with CMake's file-based API and
# lowers the reply into ordinary cmake_project and cmake_test targets. Every
# path the reply reports is made relative to the source or build directory so
# emitted targets do not depend on where the checkout lives.

_CMAKE_PRODUCT_TYPES = ["STATIC_LIBRARY", "SHARED_LIBRARY", "MODULE_LIBRARY", "EXECUTABLE"]

def _cmake_native_env():
    environment = {}
    for name in ["HOME", "PATH", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL"]:
        value = host_env(name)
        if value:
            environment[name] = value
    return environment

def _cmake_native_relative(path, root):
    if path == root:
        return "."
    if path.startswith(root + "/"):
        return path[len(root) + 1:]
    return None

# Classify one reported path as a source-tree path, a build-tree path, or
# neither. Relative paths are relative to the top-level source directory. The
# build directory is checked first because discovery places it inside the
# source tree, and anything else under Struo's own state directory, such as
# the build trees of earlier builds that a recursive glob found, is not a
# project source.
def _cmake_native_place(path, source, build):
    if not path.startswith("/") and not (len(path) > 1 and path[1] == ":"):
        path = source + "/" + path
    relative = _cmake_native_relative(path, build)
    if relative != None:
        return ("build", relative)
    if _cmake_native_relative(path, workspace_root() + "/.struo") != None:
        return ("external", path)
    relative = _cmake_native_relative(path, source)
    if relative != None:
        return ("source", relative)
    return ("external", path)

def _cmake_native_unclaimed_repair(package):
    return "If this CMake project is not part of the build, exclude it in the root struo.toml with `[workspace]` `exclude = [\"" + (package or "CMakeLists.txt") + "\"]`. Otherwise declare its seed with `kind = \"cmake_native_workspace\"` and the `configure_args` it needs."

# Variables that describe the calling shell rather than anything CMake reads.
_CMAKE_NATIVE_SHELL_VARIABLES = ["PWD", "OLDPWD", "SHLVL", "_", "TERM", "COLUMNS", "LINES", "SSH_TTY", "SSH_CLIENT", "SSH_CONNECTION"]

_CMAKE_NATIVE_QUERIES = ["codemodel-v2", "cmakeFiles-v1", "toolchains-v1"]

def _cmake_native_read_reply(reply):
    indexes = sorted([name for name in host_read_dir(reply) if name.startswith("index-") and name.endswith(".json")]) if host_path_exists(reply) else []
    if not indexes:
        return None
    index = json_decode(host_file_read(reply + "/" + indexes[-1])).get("reply") or {}
    objects = {}
    for kind in _CMAKE_NATIVE_QUERIES:
        entry = index.get(kind) or {}
        if not entry.get("jsonFile"):
            return None
        objects[kind] = json_decode(host_file_read(reply + "/" + entry["jsonFile"]))
    return objects

# Names everything a configuration read: the configure command and its
# environment, every file CMake reports reading, including CMake's own
# modules and files outside the project, the compilers it selected, and the
# list of files in the source tree, which changes when a source a glob would
# find is added or removed.
def _cmake_native_key(command, env, source, package, objects, tools):
    files = list(tools)
    for entry in objects["cmakeFiles-v1"].get("inputs") or []:
        path = entry.get("path") or ""
        if path and not entry.get("isGenerated"):
            files.append(path if path.startswith("/") or (len(path) > 1 and path[1] == ":") else source + "/" + path)
    for toolchain in objects["toolchains-v1"].get("toolchains") or []:
        compiler = (toolchain.get("compiler") or {}).get("path")
        if compiler:
            files.append(compiler)
    digests = [[path, host_file_sha256(path) if host_file_exists(path) else ""] for path in sorted(_unique(files))]
    listing = walk_workspace_files(package or ".", excluded_names = [".git", ".struo", "build", "out"])
    environment = sorted([line for line in host_command([command[0], "-E", "environment"]).split("\n") if line and line.split("=", 1)[0] not in _CMAKE_NATIVE_SHELL_VARIABLES])
    return content_sha256(_json_encode([command, env, environment, digests, listing]))

def _cmake_native_reply(cmake, source, build, build_type, configure_args, program, package):
    command = [cmake, "-S", source, "-B", build, "-G", "Ninja", "-DCMAKE_MAKE_PROGRAM:FILEPATH=" + program] + (["-DCMAKE_BUILD_TYPE=" + build_type] if build_type else []) + configure_args
    env = _cmake_native_env()
    reply = build + "/.cmake/api/v1/reply"
    stamp = build + "/struo-discovery-"

    # The tree of an earlier load is reused only when nothing its
    # configuration read has changed since; otherwise its cache, which
    # records earlier options and compilers, is discarded.
    previous = _cmake_native_read_reply(reply) if host_path_exists(build) else None
    if previous and host_path_exists(stamp + _cmake_native_key(command, env, source, package, previous, [cmake, program])):
        return (reply, previous)

    host_command([cmake, "-E", "rm", "-rf", build])
    query = build + "/.cmake/api/v1/query"
    host_command([cmake, "-E", "make_directory", query])
    host_command([cmake, "-E", "touch"] + [query + "/" + kind for kind in _CMAKE_NATIVE_QUERIES])
    output = host_command(command, cwd = source, env = env, check = False, merge_stderr = True)
    objects = _cmake_native_read_reply(reply)
    if not objects:
        lines = output.strip().split("\n")
        _diagnostic_fail("cmake.configure_failed", "configure_args", "CMake could not configure `" + (package or ".") + "`:\n" + "\n".join(lines[-40:]), _cmake_native_unclaimed_repair(package))
    host_command([cmake, "-E", "touch", stamp + _cmake_native_key(command, env, source, package, objects, [cmake, program])])
    return (reply, objects)

def _cmake_native_target(reply, record, source, build, name):
    target = json_decode(host_file_read(reply + "/" + record["jsonFile"]))
    sources = []
    headers = []
    generated_headers = []
    listed = []
    for entry in target.get("sources") or []:
        place, path = _cmake_native_place(entry.get("path") or "", source, build)
        is_header = len(_filter_by_extensions([path], _CMAKE_HEADER_EXTENSIONS)) > 0
        if place == "source":
            sources.append(path)
            if entry.get("compileGroupIndex") == None:
                listed.append(path)
            if is_header:
                headers.append(path)
        elif place == "build" and is_header:
            generated_headers.append(path)
    includes = []
    generated_includes = []
    defines = []
    for group in target.get("compileGroups") or []:
        defines.extend([define.get("define") or "" for define in group.get("defines") or []])
        for include in group.get("includes") or []:
            place, path = _cmake_native_place(include.get("path") or "", source, build)
            if place == "source":
                includes.append(path)
            elif place == "build" and [header for header in generated_headers if path == "." or header.startswith(path + "/")]:
                generated_includes.append(path)
    products = []
    for artifact in target.get("artifacts") or []:
        path = artifact.get("path") or ""
        place, path = _cmake_native_place(path, source, build) if path.startswith("/") else ("build", path)
        if place == "build" and path != ".":
            products.append(path)
    return {
        "name": name,
        "id": target.get("id") or record.get("id") or name,
        "dependencies": [dependency.get("id") for dependency in target.get("dependencies") or [] if dependency.get("id")],
        "cmake_name": target.get("name") or name,
        "type": target.get("type") or "",
        "products": products,
        "sources": sources,
        "listed": listed,
        "headers": headers,
        "generated_headers": generated_headers,
        "includes": _unique(includes),
        "defines": _unique(defines),
        "generated_includes": _unique(generated_includes),
    }

# One Ninja query answer: the rule and explicit inputs of the edge that
# produces `path`, and the outputs of the edges that read it.
def _cmake_native_ninja_query(program, build, path):
    rule = ""
    inputs = []
    outputs = []
    section = None
    for line in host_command([program, "-C", build, "-t", "query", path], env = _cmake_native_env()).split("\n"):
        stripped = line.strip()
        if line.startswith("  input:"):
            section = inputs
            rule = stripped[len("input:"):].strip()
        elif line.startswith("  outputs:"):
            section = outputs
        elif line.startswith("    ") and section != None and stripped and not stripped.startswith("|"):
            section.append(stripped)
    return (rule, inputs, outputs)

# A versioned shared library product is the development link, such as
# `libz.so`, to a real file that the runtime finds by another link named after
# its SONAME, such as `libz.so.1`. CMake's file-based API reports only the
# first, so the other links CMake creates for the same file are read from the
# generated Ninja graph and staged too.
def _cmake_native_library_links(program, build, product, symlinks):
    if product not in symlinks:
        return []
    rule, inputs, _ = _cmake_native_ninja_query(program, build, product)
    if len(inputs) != 1 or not rule.startswith("CMAKE_SYMLINK"):
        return []
    _, _, readers = _cmake_native_ninja_query(program, build, inputs[0])
    return sorted([link for link in readers if link != product and link in symlinks])

# CTest properties whose tests must not share a run with others, or need
# resources CTest schedules itself. A project that uses them keeps one suite.
_CMAKE_NATIVE_SERIAL_PROPERTIES = ["RUN_SERIAL", "RESOURCE_LOCK", "RESOURCE_GROUPS"]

def _cmake_native_test_name(test_name, names):
    base = "ctest__"
    for character in test_name.elems():
        base += character if character.isalnum() or character in "_-." else "_"
    name = base
    suffix = 2
    for _ in range(len(names) + 1):
        if name not in names:
            break
        name = base + "_" + str(suffix)
        suffix += 1
    return name

def _cmake_true(value):
    return str(value).upper() in ["1", "ON", "YES", "TRUE", "Y"]

# Values a test may use to locate a file: the whole value, the part after
# `NAME=`, and each `:`-separated part, as environment modifications such as
# `PATH=path_list_prepend:/dir` spell them. Quotes around a compile
# definition's value are dropped.
def _cmake_native_path_candidates(value):
    value = str(value)
    candidates = [value]
    if "=" in value:
        candidates.append(value.split("=", 1)[1].strip("\""))
    candidates.extend(value.split(":"))
    return candidates

# The files each CTest test reads: the project's configuration, the sources of
# every target whose artifact its command, environment, working directory,
# required files, or the compile definitions of the program it runs name,
# together with everything those targets depend on, and source-tree paths
# named in the same places. A test whose program is not a target artifact,
# such as a script or interpreter, reads every project input.
#
# Tests that share a fixture or are ordered by DEPENDS are kept together in one
# group, because CTest orders them only within one run. Disabled tests are not
# emitted. Returns a list of (test names, inputs, processors), or None when the
# project relies on CTest scheduling tests itself or names two tests the same.
# Source-tree files that targets list without compiling them: headers of any
# extension, and data. A file no target lists has no owner, and
# affected-tests already selects every test for it.
def _cmake_native_listed(every):
    listed = []
    for target in every:
        listed.extend(target["listed"])
    return sorted(_unique(listed))

# The sources of the given targets and everything they depend on, with the
# listed files under their source-tree include directories, which reach a
# compile even when the target that lists them, such as a compiled library
# beside a header-only one, is not a dependency. Returns the files and the
# targets reached.
def _cmake_native_closure(pending, by_id, listed):
    files = []
    seen = {}
    for _ in range(len(by_id) + 1):
        if not pending:
            break
        following = []
        for target_id in pending:
            if target_id in seen or target_id not in by_id:
                continue
            seen[target_id] = True
            files.extend(by_id[target_id]["sources"])
            # A quoted include also resolves beside the including source.
            siblings = [path.rsplit("/", 1)[0] if "/" in path else "." for path in by_id[target_id]["sources"]]
            for include in _unique(by_id[target_id]["includes"] + siblings):
                prefix = "" if include == "." else include + "/"
                files.extend([path for path in listed if path.startswith(prefix)])
            following.extend(by_id[target_id]["dependencies"])
        pending = following
    return (files, seen)

def _cmake_native_test_plan(tests, every, config_inputs, project_inputs, source, build):
    by_id = {target["id"]: target for target in every}
    by_artifact = {}
    for target in every:
        for product in target["products"]:
            by_artifact[build + "/" + product] = target
    names = [test.get("name") or "" for test in tests]
    if len(_unique(names)) != len(names):
        return None
    group_of = {name: name for name in names}

    def find(name):
        for _ in range(len(names) + 1):
            if group_of[name] == name:
                return name
            name = group_of[name]
        return name

    def union(left, right):
        left = find(left)
        right = find(right)
        if left != right:
            group_of[right] = left

    # Only headers the project lists: a header no target lists has no owner,
    # and affected-tests already selects every test for it.
    project_headers = _cmake_native_listed(every)
    fixture_users = {}
    reached = {}
    reads = {}
    processors = {}
    enabled = []
    for test in tests:
        name = test.get("name") or ""
        properties = {entry.get("name"): entry.get("value") for entry in test.get("properties") or []}
        for key in _CMAKE_NATIVE_SERIAL_PROPERTIES:
            if properties.get(key):
                return None
        if _cmake_true(properties.get("DISABLED") or ""):
            continue
        command = test.get("command") or []
        if not command:
            return None
        enabled.append(name)
        for fixture in _cmake_list(properties.get("FIXTURES_SETUP")) + _cmake_list(properties.get("FIXTURES_CLEANUP")) + _cmake_list(properties.get("FIXTURES_REQUIRED")):
            fixture_users.setdefault(fixture, []).append(name)
        for dependency in _cmake_list(properties.get("DEPENDS")):
            if dependency in group_of:
                union(name, dependency)
        processors[name] = int(properties.get("PROCESSORS") or 1)

        files = list(config_inputs)
        whole = command[0] not in by_artifact
        values = list(command) + [properties.get("WORKING_DIRECTORY") or ""] + _cmake_list(properties.get("REQUIRED_FILES")) + _cmake_list(properties.get("ENVIRONMENT")) + _cmake_list(properties.get("ENVIRONMENT_MODIFICATION"))
        if command[0] in by_artifact:
            values.extend(by_artifact[command[0]]["defines"])
        pending = []
        for value in values:
            for candidate in _cmake_native_path_candidates(value):
                if candidate in by_artifact:
                    pending.append(by_artifact[candidate]["id"])
                    continue
                if not candidate.startswith("/"):
                    continue
                place, path = _cmake_native_place(candidate, source, build)
                if place != "source":
                    continue
                if path == ".":
                    whole = True
                elif host_path_exists(source + "/" + path) and not host_file_exists(source + "/" + path):
                    files.append(path + "/**")
                else:
                    files.append(path)
        closure, seen = _cmake_native_closure(pending, by_id, project_headers)
        files.extend(closure)
        reached.update(seen)
        reads[name] = project_inputs if whole else files
    for users in fixture_users.values():
        for user in users[1:]:
            union(users[0], user)

    # A custom target, such as one copying test data into the build tree,
    # runs with the whole build whether or not a test depends on it, so its
    # sources are read by every test that does not already reach it.
    unreached = []
    for target in every:
        if target["type"] == "UTILITY" and target["id"] not in reached:
            unreached.extend(target["sources"])
    if unreached:
        for name in list(reads.keys()):
            reads[name] = list(reads[name]) + unreached

    groups = {}
    order = []
    for name in enabled:
        root = find(name)
        if root not in groups:
            groups[root] = []
            order.append(root)
        groups[root].append(name)
    plan = []
    for root in order:
        members = groups[root]
        files = []
        for member in members:
            files.extend(reads[member])
        plan.append((members, sorted(_unique(files)), max([processors[member] for member in members])))
    return plan

def _cmake_list(value):
    if not value:
        return []
    if type(value) == "list":
        return [str(item) for item in value]
    return [item for item in str(value).split(";") if item]

def _cmake_native_resolver(ctx):
    attrs = ctx["attr"]
    label = ctx["label"]["id"]
    cmake = _resolve_host_executable(attrs.get("cmake") or "cmake")
    ctest = _resolve_host_executable(attrs.get("ctest") or "ctest")
    program = _resolve_host_executable("ninja")
    if not cmake or not ctest or not program:
        _diagnostic_fail("cmake.tools_missing", "cmake", label + ": CMake discovery requires `cmake`, `ctest`, and `ninja` on PATH", ["Install CMake and Ninja.", _cmake_native_unclaimed_repair(ctx["label"]["package"])])
    build_type = attrs.get("build_type") or ""
    configure_args = attrs.get("configure_args") or []
    package = ctx["label"]["package"]
    source = workspace_root() + ("/" + package if package else "")
    build = workspace_root() + "/.struo/tmp/cmake-discovery/" + (package.replace("/", "__") if package else "_root")
    reply, objects = _cmake_native_reply(cmake, source, build, build_type, configure_args, program, package)
    codemodel = objects["codemodel-v2"]
    paths = codemodel.get("paths") or {}
    source = paths.get("source") or source
    build = paths.get("build") or build

    project_inputs = []
    for entry in objects["cmakeFiles-v1"].get("inputs") or []:
        if entry.get("isExternal") or entry.get("isCMake") or entry.get("isGenerated"):
            continue
        place, path = _cmake_native_place(entry.get("path") or "", source, build)
        if place == "source":
            project_inputs.append(path)
            # Recorded so a reused graph is derived again when a file CMake
            # read while configuring changes, such as a configure_file input.
            host_file_sha256(source + "/" + path)

    configurations = codemodel.get("configurations") or []
    if len(configurations) != 1:
        fail(label + ": CMake discovery expects one configuration from the Ninja generator, got " + str(len(configurations)))
    config_inputs = sorted(_unique(project_inputs))
    lowered = []
    every = []
    names = {"ctest": "the CTest suite", "ctest_build": "the CTest build tree", "cmake_build": "the CMake product build tree", ctx["label"]["name"]: "the discovered CMake project"}
    for record in sorted(configurations[0].get("targets") or [], key = lambda record: record.get("name") or ""):
        target = _cmake_native_target(reply, record, source, build, record.get("name") or "")
        every.append(target)
        project_inputs.extend(target["sources"])
        if not target["products"] or target["type"] not in _CMAKE_PRODUCT_TYPES:
            continue
        if target["name"] in names:
            _diagnostic_fail("cmake.target_name_conflict", "", label + ": CMake target `" + target["name"] + "` has the same name as " + names[target["name"]] + ".", "Declare this CMake project in struo.toml with cmake_project and cmake_test targets under other names.")
        names[target["name"]] = "CMake target `" + target["name"] + "`"
        lowered.append(target)
    symlinks = {}
    if [target for target in lowered if target["type"] == "SHARED_LIBRARY"]:
        for line in host_command([program, "-C", build, "-t", "targets", "rule", "CMAKE_SYMLINK_LIBRARY"], env = _cmake_native_env()).split("\n"):
            if line.strip():
                symlinks[line.strip()] = True
    for target in lowered:
        if target["type"] == "SHARED_LIBRARY":
            for product in list(target["products"]):
                target["products"] = _unique(target["products"] + _cmake_native_library_links(program, build, product, symlinks))
    project_inputs = sorted(_unique(project_inputs))

    specs = []
    by_id = {target["id"]: target for target in every}
    listed = _cmake_native_listed(every)
    if lowered:
        # One incremental tree builds every product; each product target copies
        # its files out of it and reads only what its CMake target reaches.
        build_attrs = {"build_type": build_type, "configure_args": configure_args, "export_build_tree": True, "build_targets": [target["cmake_name"] for target in lowered], "_affected_srcs": []}
        if attrs.get("cmake"):
            build_attrs["cmake"] = attrs["cmake"]
        specs.append({"name": "cmake_build", "kind": "cmake_project", "deps": [], "srcs": project_inputs, "attrs": build_attrs})
    for target in lowered:
        target_attrs = {
            "build_type": build_type,
            "configure_args": configure_args,
            "build_targets": [target["cmake_name"]],
            "products": target["products"] + (target["generated_headers"] if target["generated_includes"] else []),
            "hdrs": target["headers"],
            "includes": target["includes"],
            "generated_includes": target["generated_includes"],
        }
        if attrs.get("cmake"):
            target_attrs["cmake"] = attrs["cmake"]
        reached, _ = _cmake_native_closure([target["id"]], by_id, listed)
        specs.append({"name": target["name"], "kind": "cmake_project", "deps": ["./cmake_build"], "srcs": sorted(_unique(config_inputs + reached)), "attrs": target_attrs})
    roots = [spec["name"] for spec in specs if spec["name"] != "cmake_build"]

    # CTest leaves out the command of a test whose executable does not exist,
    # and the discovery tree is never built, so empty files stand in for the
    # artifacts.
    artifacts = [build + "/" + product for target in every if target["type"] == "EXECUTABLE" for product in target["products"]]
    if artifacts:
        host_command([cmake, "-E", "make_directory"] + _unique([path.rsplit("/", 1)[0] for path in artifacts]))
        host_command([cmake, "-E", "touch"] + artifacts)
    tests = json_decode(host_command([ctest, "--test-dir", build, "--show-only=json-v1"], cwd = source, env = _cmake_native_env())).get("tests") or []
    test_roots = []
    if tests:
        common = {"build_type": build_type, "configure_args": configure_args}
        if attrs.get("cmake"):
            common["cmake"] = attrs["cmake"]
        plan = _cmake_native_test_plan(tests, every, config_inputs, project_inputs, source, build)
        if plan == None:
            test_attrs = dict(common, jobs = attrs.get("jobs") or 4)
            if attrs.get("ctest"):
                test_attrs["ctest"] = attrs["ctest"]
            specs.append({"name": "ctest", "kind": "cmake_test", "deps": [], "srcs": project_inputs, "attrs": test_attrs})
            test_roots.append("ctest")
        else:
            specs.append({"name": "ctest_build", "kind": "cmake_project", "deps": [], "srcs": project_inputs, "attrs": dict(common, export_build_tree = True, native_paths = True, _affected_srcs = [])})
            for test_names, inputs, jobs in plan:
                name = _cmake_native_test_name(test_names[0], names)
                names[name] = "CTest test `" + test_names[0] + "`"
                test_attrs = dict(common, tests = test_names, jobs = max(jobs, 1))
                if attrs.get("ctest"):
                    test_attrs["ctest"] = attrs["ctest"]
                specs.append({"name": name, "kind": "cmake_test", "deps": ["./ctest_build"], "srcs": inputs, "attrs": test_attrs})
                test_roots.append(name)
    return {
        "targets": specs,
        "roots": roots,
        "attrs": {
            "_cmake_resolved": True,
            "_default_test_roots": test_roots,
        },
    }

def _cmake_native_impl(ctx):
    if not _cmake_attr(ctx, "_cmake_resolved", False):
        fail(ctx["label"]["id"] + ": cmake_native_workspace must be expanded by its graph resolver before analysis")
    return {
        "label_id": ctx["label"]["id"],
        "target_kind": "cmake_native_workspace",
        "cmake_native_workspace": True,
        "targets": ctx["deps"],
    }

_CMAKE_EXAMPLES = [
    example(
        "cmake-project-minimal",
        name = "CMake static library bridge",
        use_when = "Use this when CMake remains authoritative while Struo builds declared products and imports a checked logical target snapshot.",
    ),
]

_CMAKE_SOURCE_REFERENCES = [
    source_reference(
        "CMake",
        "cmake",
        "https://cmake.org/cmake/help/latest/manual/cmake.1.html",
        "Use this for the authoritative configure and generated-build invocation contract.",
    ),
    source_reference(
        "CMake",
        "file-based application programming interface",
        "https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html",
        "Use this to import configured targets, dependencies, sources, artifacts, and toolchain metadata without parsing CMakeLists.txt.",
    ),
]

cmake_project = target_kind(
    docs = "Coarse bridge that keeps CMake authoritative, runs uncached configure and build, stages only declared products, and exposes native link providers. Compiler selection remains native to CMake and is not assumed from default host tools.",
    attrs = [
        attr("cmake", "string", default = "\"cmake\"", docs = "CMake executable name, absolute path, or workspace-relative path.", configurable = False),
        attr("source_dir", "string", default = "\".\"", docs = "Package-relative CMake source directory.", configurable = False),
        attr("generator", "string", default = "\"Ninja\"", docs = "CMake generated-build backend.", configurable = False),
        attr("build_program", "string", docs = "Generated-build executable. Defaults to resolving `ninja` for the Ninja generator.", configurable = False),
        attr("build_type", "string", default = "\"Debug\"", docs = "CMake build configuration passed during configure and build."),
        attr("configure_args", "list<string>", default = "[]", docs = "Additional arguments passed to CMake configuration. Each item is one argument, so `-DNAME=a;b` sets a CMake list.", configurable = False),
        attr("build_targets", "list<string>", default = "[]", docs = "Optional native CMake targets passed to the generated build.", configurable = False),
        attr("build_args", "list<string>", default = "[]", docs = "Additional arguments passed to `cmake --build`, each item as one argument.", configurable = False),
        attr("products", "list<string>", default = "[]", docs = "Files expected under the generated build directory. Product tokens support static, shared, and executable platform suffixes. Required unless export_build_tree is set.", configurable = False),
        attr("native_paths", "bool", default = "false", docs = "Keep the compiler's native source and build paths instead of mapping the workspace path to `.`. Set it on an exported tree that CTest runs in, whose tests may find their data through `__FILE__`.", configurable = False),
        attr("export_build_tree", "bool", default = "false", docs = "Declare the whole generated build directory as an output and expose it to dependents: cmake_test targets run CTest in it, and cmake_project targets copy their products from it, instead of each building their own. The tree is built incrementally and selects no tests itself.", configurable = False),
        attr("_affected_srcs", "list<string>", docs = "Resolver-owned globs, relative to the package, of the files that select this target's dependent tests in `struo query affected-tests`, matched instead of `srcs`.", configurable = False),
        attr("hdrs", "list<string>", default = "[]", docs = "Public package-relative headers exposed to native dependents.", configurable = False),
        attr("header_globs", "list<string>", default = "[]", docs = "Public header glob patterns exposed to native dependents.", configurable = False),
        attr("includes", "list<string>", default = "[]", docs = "Public include directories propagated to native dependents.", configurable = False),
        attr("generated_includes", "list<string>", default = "[]", docs = "Public include directories relative to the generated build directory, for headers CMake generates. Declared header products under each directory, or a product naming the directory itself, are published as headers.", configurable = False),
        attr("defines", "list<string>", default = "[]", docs = "Preprocessor definitions propagated to native dependents.", configurable = False),
        attr("linkopts", "list<string>", default = "[]", docs = "Linker options propagated to native dependents.", configurable = False),
        attr("data", "list<string>", default = "[]", docs = "Runtime data globs propagated to dependents.", configurable = False),
        attr("env", "map<string,string>", default = "{}", docs = "Environment variables passed to CMake configuration and build.", configurable = False),
    ],
    deps = [dep("deps", ["c_provider"], "Native dependency files included in the CMake action input tree. A cmake_project with export_build_tree among them supplies the build tree this target copies its products from instead of building.")],
    providers = ["cmake_project", "c_provider", "native_linkable", "apple_linkable"],
    capabilities = [capability("build", ["default", "products", "library"])],
    tools = [tool("cmake")],
    examples = _CMAKE_EXAMPLES,
    source_references = _CMAKE_SOURCE_REFERENCES,
    impl = _cmake_project_impl,
)

cmake_test = target_kind(
    docs = "Builds and runs a CTest suite or explicit native CMake test targets uncached at its original local workspace path, preserving native compiler selection. Reports suite-level results, native JUnit for CTest, and normalized failure on timeout.",
    attrs = [
        attr("cmake", "string", default = "\"cmake\"", docs = "CMake executable name or path.", configurable = False),
        attr("ctest", "string", default = "\"ctest\"", docs = "CTest executable name or path, requiring CTest 3.21 or newer.", configurable = False),
        attr("source_dir", "string", default = "\".\"", docs = "Package-relative CMake source directory.", configurable = False),
        attr("generator", "string", default = "\"Ninja\"", docs = "CMake generated-build backend.", configurable = False),
        attr("build_program", "string", docs = "Generated-build executable. Defaults to ninja for Ninja.", configurable = False),
        attr("build_type", "string", default = "\"Debug\"", docs = "Build configuration, preserving native debug information."),
        attr("configure_args", "list<string>", default = "[]", docs = "Additional CMake configure arguments. Enable upstream tests here. Each item is one argument, so `-DNAME=a;b` sets a CMake list.", configurable = False),
        attr("build_targets", "list<string>", default = "[]", docs = "Optional native build targets. Defaults to building all targets, including test executables.", configurable = False),
        attr("tests", "list<string>", default = "[]", docs = "CTest test names to run, matched exactly. Empty runs every test.", configurable = False),
        attr("test_targets", "list<string>", default = "[]", docs = "Native CMake targets to execute as the test suite through cmake --build. Empty uses CTest with no-tests=error. Explicit targets own test discovery and failure reporting; no native JUnit is assumed.", configurable = False),
        attr("jobs", "int", default = "1", docs = "Native build and CTest concurrency, reserved through the action resource pool.", configurable = False),
        attr("data", "list<string>", default = "[]", docs = "Runtime and configuration input globs.", configurable = False),
        attr("env", "map<string,string>", default = "{}", docs = "Declared build and test environment.", configurable = False),
        attr("coverage_per_test", "bool", default = "false", docs = "Measure isolated registered CTest invocations with LLVM coverage. Each invocation is one atomic execution unit, not its internal framework cases. Requires Linux strace and Python 3.", configurable = False),
        attr("coverage_sources", "list<string>", default = "[]", docs = "Instrumented product source globs. Other declared inputs bind test inventory and configuration; do not include test sources here.", configurable = False),
        attr("llvm_cov", "string", default = "\"llvm-cov\"", docs = "Compiler-matching LLVM coverage exporter.", configurable = False),
        attr("llvm_profdata", "string", default = "\"llvm-profdata\"", docs = "Compiler-matching LLVM profile merger.", configurable = False),
        attr("labels", "list<string>", default = "[]", docs = "Labels exposed through test discovery.", configurable = False),
        attr("timeout_ms", "int", docs = "Optional positive suite timeout in milliseconds, excluding configure and build. A timeout retains normalized failed results; native JUnit may be incomplete.", configurable = False),
    ],
    deps = [dep("deps", ["cmake_project"], "A cmake_project with export_build_tree whose generated build directory CTest runs in, instead of configuring and building a tree for this target.")],
    providers = ["struo_test_info"],
    capabilities = [capability("build", ["default"]), capability("test", ["default"]), capability("test-discovery", ["test_inventory"])],
    tools = [tool("cmake"), tool("ctest")],
    examples = [example("cmake-test-minimal", name = "CTest suite", use_when = "Keep an existing CMake test suite authoritative and report one normalized suite with native JUnit.")],
    source_references = _CMAKE_SOURCE_REFERENCES,
    impl = _cmake_test_impl,
)

cmake_workspace = target_kind(
    docs = "Imports a checked CMake file-interface snapshot as queryable synthetic targets while leaving execution with cmake_project.",
    attrs = [
        attr("snapshot", "string", required = True, docs = "Package-relative normalized CMake snapshot with exact resolver-input provenance.", configurable = False),
        attr("resolver_inputs", "list<string>", default = "[]", docs = "Package-relative text globs supplied to the resolver. Include the snapshot and every bound configuration input.", configurable = False),
        attr("source_dir", "string", default = "\".\"", docs = "Package-relative source directory recorded in snapshot selection provenance.", configurable = False),
        attr("generator", "string", default = "\"Ninja\"", docs = "CMake generated-build backend recorded in snapshot selection provenance.", configurable = False),
        attr("build_type", "string", default = "\"Debug\"", docs = "CMake build configuration recorded in snapshot selection provenance."),
        attr("exports", "list<string>", default = "[]", docs = "CMake names or generated Struo names to expose. Defaults to snapshot exports.", configurable = False),
        attr("_cmake_resolved", "bool", default = "false", docs = "Resolver-owned marker indicating that the snapshot was expanded.", configurable = False),
        attr("_cmake_snapshot_fingerprint", "string", docs = "Resolver-owned normalized snapshot fingerprint.", configurable = False),
        attr("_cmake_exports", "list<string>", default = "[]", docs = "Resolver-owned generated export names.", configurable = False),
    ],
    deps = [dep("deps", ["cmake_target"], "Resolver-generated logical CMake targets.")],
    providers = ["cmake_workspace"],
    examples = _CMAKE_EXAMPLES,
    source_references = _CMAKE_SOURCE_REFERENCES,
    resolver = _cmake_workspace_resolver,
    impl = _cmake_workspace_impl,
)

cmake_target = target_kind(
    docs = "Resolver-generated logical CMake target used for graph queries, dependency explanations, and affected-input reasoning.",
    attrs = [
        attr("cmake_name", "string", required = True, docs = "Original configured CMake target name.", configurable = False),
        attr("cmake_type", "string", required = True, docs = "Configured CMake target type.", configurable = False),
        attr("artifacts", "list<string>", default = "[]", docs = "Normalized artifacts reported by CMake.", configurable = False),
        attr("include_dirs", "list<string>", default = "[]", docs = "Normalized include directories reported by CMake.", configurable = False),
        attr("compile_definitions", "list<string>", default = "[]", docs = "Configured compile definitions reported by CMake.", configurable = False),
        attr("snapshot_fingerprint", "string", required = True, docs = "Normalized snapshot fingerprint shared by imported targets.", configurable = False),
        attr("_cmake_resolved", "bool", default = "false", docs = "Resolver-owned marker preventing direct manifest authoring.", configurable = False),
    ],
    deps = [dep("deps", ["cmake_target"], "Configured logical CMake target dependencies.")],
    providers = ["cmake_target"],
    examples = _CMAKE_EXAMPLES,
    source_references = _CMAKE_SOURCE_REFERENCES,
    impl = _cmake_target_impl,
)

cmake_native_workspace = target_kind(
    docs = "Native CMake project seed. Configures the project once with the Ninja generator and CMake's file-based API, then emits one cmake_project per configured library or executable and one cmake_test for the CTest suite. CMake stays authoritative for building.",
    attrs = [
        attr("cmake", "string", docs = "CMake executable name or path. Defaults to resolving `cmake`.", configurable = False),
        attr("ctest", "string", docs = "CTest executable name or path. Defaults to resolving `ctest`.", configurable = False),
        attr("build_type", "string", default = "\"\"", docs = "Build configuration used for discovery and passed to emitted targets. Empty keeps CMake's default, which adds no optimization or debug flags.", configurable = False),
        attr("configure_args", "list<string>", default = "[]", docs = "Additional CMake configure arguments used for discovery and passed to emitted targets.", configurable = False),
        attr("jobs", "int", default = "4", docs = "Concurrency for the emitted CTest suite.", configurable = False),
        attr("resolver_inputs", "list<string>", default = "[]", docs = "Package-relative text globs supplied to native discovery. Defaults to srcs when empty.", configurable = False),
        attr("_cmake_resolved", "bool", default = "false", docs = "Resolver-owned marker indicating that CMake targets were expanded.", configurable = False),
        attr("_default_test_roots", "list<string>", default = "[]", docs = "Resolver-owned first-party test target names used by targetless test selection.", configurable = False),
    ],
    deps = [dep("deps", ["cmake_project"], "Resolver-generated CMake products.")],
    providers = ["cmake_native_workspace"],
    capabilities = [capability("build", [])],
    tools = [tool("cmake"), tool("ctest")],
    examples = [example("cmake-native-project", name = "CMake project without a Struo manifest", use_when = "Use this when an existing CMake project should be built and tested through Struo without declaring targets.")],
    source_references = _CMAKE_SOURCE_REFERENCES,
    resolver = _cmake_native_resolver,
    impl = _cmake_native_impl,
)

cmake = native_project(
    target_kind = "cmake_native_workspace",
    docs = "Recognizes a native CMake project from its top-level CMakeLists.txt.",
    markers = ["CMakeLists.txt"],
    target_name = "cmake",
    inputs = ["**/CMakeLists.txt", "**/*.cmake", "CMakePresets.json"],
    manifests = ["CMakeLists.txt", "**/CMakeLists.txt", "**/*.cmake", "CMakePresets.json"],
    exclude = _native_project_generated_dirs() + ["build", "out", "node_modules"],
    input_exclude = ["build", "out", ".git", ".struo"],
    on_match = "stop",
    requires_tools = ["cmake", "ctest", "ninja"],
    claimed_by = ["cmake_project", "cmake_test"],
)
