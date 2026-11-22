# From the checkout root: rig run --rscript scripts/generate-notices.R [--check]
# Cargo fetches locked dependencies normally. Other upstream inputs are pinned
# by digest and cached in the maintainer's Cargo home; end users do not run this.
command_output <- function(program, args) {
  result <- system2(program, shQuote(args), stdout = TRUE)
  if (!is.null(attr(result, "status"))) {
    stop(program, " failed: ", paste(args, collapse = " "))
  }
  result
}

sha <- function(path) unname(tools::sha256sum(path))

verified_download <- function(url, expected) {
  cache <- file.path(cargo_home, "shinytauri-notice-inputs")
  dir.create(cache, showWarnings = FALSE, recursive = TRUE)
  cached <- file.path(cache, expected)
  if (!file.exists(cached)) {
    tmp <- tempfile(tmpdir = cache)
    on.exit(unlink(tmp))
    command_output(
      "curl",
      c(
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--retry",
        "2",
        "--output",
        tmp,
        url
      )
    )
    if (sha(tmp) != expected) {
      stop("Upstream checksum changed: ", url)
    }
    if (!file.rename(tmp, cached) && !file.exists(cached)) {
      stop("Cannot cache ", url)
    }
  }
  if (sha(cached) != expected) {
    stop("Incorrect cached checksum for ", url)
  }
  cached
}

read_text <- function(path) {
  text <- rawToChar(readBin(path, "raw", n = file.size(path)))
  Encoding(text) <- "UTF-8"
  if (!nzchar(text) || !validUTF8(text)) {
    stop("Expected nonempty UTF-8 notice: ", path)
  }
  gsub("\r\n", "\n", text, fixed = TRUE)
}

# These published .crate files omit their upstream license text. Pin each
# exception to the reviewed crate version and corresponding source revision.
missing_text <- list(
  "alloc-stdlib-0.3.0" = c(
    "https://raw.githubusercontent.com/dropbox/rust-alloc-no-stdlib/0a81fd6928ea3b33c8cd484aa4575d50ffb98012/LICENSE",
    "c0c56f26d9c051cac4d200c34c84e7ae9aaa853e01a982a1df08b09931e518ae"
  ),
  "webview2-com-0.39.1" = c(
    "https://raw.githubusercontent.com/wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE",
    "0dcf41516e608bbcb6cdc5229feb7b86fe4a643b85e7df251133c93408fdac73"
  ),
  "selectors-0.38.0" = c(
    "https://www.mozilla.org/media/MPL/2.0/index.815ca599c9df.txt",
    "fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85"
  )
)
missing_text[["webview2-com-sys-0.39.1"]] <- missing_text[[
  "webview2-com-0.39.1"
]]
missing_text[["webview2-com-macros-0.8.1"]] <- c(
  "https://raw.githubusercontent.com/wravery/webview2-rs/dffa41a8a46d3f5565eefbff2de57d38d399f158/LICENSE",
  "0dcf41516e608bbcb6cdc5229feb7b86fe4a643b85e7df251133c93408fdac73"
)

# Cargo supplies the actual target/features graph. Include build dependencies
# and procedural macros conservatively because they can emit distributed code.
package_set <- function(project, targets) {
  args <- c(
    "tree",
    "--locked",
    "--manifest-path",
    file.path(project, "Cargo.toml"),
    "--edges",
    "normal,build",
    "--prefix",
    "none",
    "--format",
    "{p}|{l}"
  )
  if (project == "src-tauri") {
    args <- c(args, "--features", "tauri/custom-protocol")
  }
  lines <- unlist(lapply(targets, function(target) {
    command_output("cargo", c(args, "--target", target))
  }))
  if (project == "src-tauri") {
    lines <- c(lines, native_windows_build_edges(project, lines))
  }
  rows <- strsplit(unique(sub(" \\(\\*\\)$", "", lines)), "|", fixed = TRUE)
  root <- RcppTOML::parseTOML(file.path(project, "Cargo.toml"))$package$name
  lock <- RcppTOML::parseTOML(file.path(project, "Cargo.lock"))$package
  names(lock) <- vapply(lock, function(p) paste0(p$name, " v", p$version), "")
  selected <- list()
  for (row in rows) {
    if (
      startsWith(row[1], paste0(root, " v")) &&
        grepl(" (", row[1], fixed = TRUE)
    ) {
      next
    }
    p <- lock[[sub(" (proc-macro)", "", row[1], fixed = TRUE)]]
    if (
      is.null(p) ||
        !identical(
          p$source,
          "registry+https://github.com/rust-lang/crates.io-index"
        ) ||
        length(row) != 2L ||
        !nzchar(row[2]) ||
        !grepl("^[0-9a-f]{64}$", p$checksum)
    ) {
      stop(
        "Review unresolved dependency or license: ",
        paste(row, collapse = "|")
      )
    }
    p$license <- row[2]
    selected[[paste0(p$name, "-", p$version)]] <- p
  }
  selected[sort(names(selected), method = "radix")]
}

# A Linux-to-Windows build resolves build dependencies for Linux. Restore
# Windows-only branches needed when those same build tools run on Windows.
# Cargo metadata can expose unrelated optional features through dev-dependency
# unification, so seed only required, target-gated edges of the active tree.
native_windows_build_edges <- function(project, active) {
  json <- command_output(
    "cargo",
    c(
      "metadata",
      "--locked",
      "--format-version",
      "1",
      "--manifest-path",
      file.path(project, "Cargo.toml"),
      "--features",
      "tauri/custom-protocol",
      "--filter-platform",
      "x86_64-pc-windows-msvc"
    )
  )
  metadata <- jsonlite::fromJSON(
    paste(json, collapse = "\n"),
    simplifyVector = FALSE
  )
  packages <- metadata$packages
  nodes <- metadata$resolve$nodes
  names(packages) <- vapply(packages, function(p) p$id, "")
  names(nodes) <- vapply(nodes, function(n) n$id, "")
  labels <- vapply(packages, function(p) paste0(p$name, " v", p$version), "")
  active_labels <- sub(
    " (proc-macro)",
    "",
    sub("\\|.*", "", active),
    fixed = TRUE
  )
  seen <- names(labels)[labels %in% active_labels]
  remaining <- character()
  for (id in seen) {
    for (edge in nodes[[id]]$deps) {
      if (edge$pkg %in% seen) {
        next
      }
      for (kind in edge$dep_kinds) {
        if (is.null(kind$target) || identical(kind$kind, "dev")) {
          next
        }
        required <- any(vapply(
          packages[[id]]$dependencies,
          function(dep) {
            identical(dep$name, packages[[edge$pkg]]$name) &&
              !dep$optional &&
              identical(dep$kind, kind$kind) &&
              identical(dep$target, kind$target)
          },
          logical(1)
        ))
        if (required) remaining <- c(remaining, edge$pkg)
      }
    }
  }
  added <- character()
  while (length(remaining)) {
    id <- remaining[[1]]
    remaining <- remaining[-1]
    if (id %in% c(seen, added)) {
      next
    }
    added <- c(added, id)
    for (edge in nodes[[id]]$deps) {
      if (
        any(vapply(
          edge$dep_kinds,
          function(k) !identical(k$kind, "dev"),
          logical(1)
        ))
      ) {
        remaining <- c(remaining, edge$pkg)
      }
    }
  }
  vapply(
    added,
    function(id) paste(labels[[id]], packages[[id]]$license, sep = "|"),
    ""
  )
}

notice_files <- function(p) {
  id <- paste0(p$name, "-", p$version)
  # Read original archive contents, not a potentially edited Cargo checkout.
  archive <- Sys.glob(file.path(
    cargo_home,
    "registry",
    "cache",
    "*",
    paste0(id, ".crate")
  ))
  archive <- archive[sha(archive) == p$checksum]
  if (!length(archive)) {
    stop("Missing checksummed crates.io archive: ", id)
  }
  crate <- tempfile()
  dir.create(crate)
  on.exit(unlink(crate, recursive = TRUE))
  if (utils::untar(archive[1], exdir = crate, tar = "internal") != 0L) {
    stop("Cannot unpack ", id)
  }
  directory <- file.path(crate, id)
  files <- list.files(directory, recursive = TRUE, all.files = TRUE)
  manifest <- RcppTOML::parseTOML(file.path(directory, "Cargo.toml"))
  pattern <- "(^|/)(licen[sc]es?|unlicense|copying|copyrights?|notices?|authors|third[ _-]?party[ _-]?(licenses?|notices?))($|[-._/])"
  paths <- sort(
    unique(c(
      grep(pattern, files, ignore.case = TRUE, value = TRUE),
      manifest$package[["license-file"]]
    )),
    method = "radix"
  )
  texts <- setNames(
    lapply(paths, function(path) read_text(file.path(directory, path))),
    paths
  )
  main_license <- grep(
    "(^|/)(licen[sc]es?|unlicense|copying)($|[-._/])",
    paths,
    ignore.case = TRUE,
    value = TRUE
  )
  if (
    !length(setdiff(
      main_license,
      grep("[.]spdx$", main_license, value = TRUE)
    )) &&
      is.null(manifest$package[["license-file"]])
  ) {
    extra <- missing_text[[id]]
    if (is.null(extra)) {
      stop("Missing upstream license; review ", id)
    }
    texts[[extra[1]]] <- read_text(verified_download(extra[1], extra[2]))
  }

  # Some upstream attribution and permission notices live inside source files.
  # Keep these verbatim rather than substituting a generic SPDX license.
  excerpts <- source_notices[[id]]
  for (path in names(excerpts)) {
    lines <- readLines(
      file.path(directory, path),
      encoding = "UTF-8",
      warn = FALSE
    )
    if (length(lines) < max(excerpts[[path]])) {
      stop("Missing source notice: ", id, "/", path)
    }
    texts[[path]] <- paste(lines[excerpts[[path]]], collapse = "\n")
  }
  if (id == "brotli-decompressor-6.3.2") {
    url <- "https://raw.githubusercontent.com/google/brotli/v1.1.0/LICENSE"
    texts[[url]] <- read_text(verified_download(
      url,
      "3d180008e36922a4e8daec11c34c7af264fed5962d07924aea928c38e8663c94"
    ))
  }
  if (p$name == "webview2-com-sys") {
    texts <- c(texts, sdk_notices(directory))
  }
  texts
}

sdk_notices <- function(directory) {
  version <- "1.0.3800.47"
  id <- "microsoft.web.webview2"
  url <- sprintf(
    "https://api.nuget.org/v3-flatcontainer/%s/%s/%s.%s.nupkg",
    id,
    version,
    id,
    version
  )
  archive <- verified_download(
    url,
    "56c9f26bdd07916a2d1949fb58a5c7e434dfa1173577dca879206050c4e718db"
  )
  dest <- tempfile()
  dir.create(dest)
  on.exit(unlink(dest, recursive = TRUE))
  binaries <- c("WebView2Loader.dll", "WebView2LoaderStatic.lib")
  upstream <- paste0("build/native/x64/", binaries)
  docs <- c("LICENSE.txt", "NOTICE.txt")
  utils::unzip(archive, c(upstream, docs), exdir = dest)
  if (
    !identical(
      sha(file.path(dest, upstream)),
      sha(file.path(directory, "x64", binaries))
    )
  ) {
    stop(
      "Review changed Microsoft WebView2 Loader binaries and their SDK notices"
    )
  }
  setNames(lapply(file.path(dest, docs), read_text), paste0(url, " / ", docs))
}

source_notices <- list(
  "tree-sitter-0.27.2" = list("src/portable/endian.h" = 1:5),
  "vswhom-sys-0.1.3" = list("ext/vswhom.cpp" = 1:8),
  "tao-0.37.3" = list("src/keyboard.rs" = 1:74),
  "find-msvc-tools-0.1.15" = list("src/com.rs" = 1:6, "src/winapi.rs" = 1:6),
  "dpi-0.1.2" = list("src/libm.rs" = 1:11),
  "dtoa-1.0.11" = list("src/dtoa.rs" = 14:27),
  "brotli-9.1.3" = list("src/enc/entropy_encode.rs" = 1:5),
  "brotli-decompressor-6.3.2" = list("src/context.rs" = 5:8),
  "cookie-0.18.2" = list("src/secure/macros.rs" = 42:43),
  "tauri-2.12.7" = list("src/resources/mod.rs" = 1L)
)

render <- function(packages, components, component_role) {
  names_by_id <- vapply(packages, function(p) paste(p$name, p$version), "")
  entries <- lapply(names(packages), function(id) {
    p <- packages[[id]]
    paste0(
      names_by_id[[id]],
      "  |  ",
      p$license,
      "\n  Source: https://static.crates.io/crates/",
      p$name,
      "/",
      id,
      ".crate"
    )
  })
  # Every original document is presented once, with all its applicable paths.
  labels <- unlist(
    lapply(names(packages), function(id) {
      paste(names_by_id[[id]], "/", names(components[[id]]))
    }),
    use.names = FALSE
  )
  bodies <- unlist(components[names(packages)], use.names = FALSE)
  distinct <- unique(bodies)
  groups <- split(
    labels,
    factor(match(bodies, distinct), levels = seq_along(distinct))
  )
  sections <- Map(
    function(names, text) paste(c(names, "", text), collapse = "\n"),
    groups,
    distinct
  )
  paste(
    c(
      paste("Third-party notices for the shinytauri", component_role),
      "Rust standard library notices accompany this file in rust-stdlib.html.",
      "The index includes locked normal and build dependencies, including procedural macros.",
      "Source links provide the unmodified source archive for each listed version.",
      "",
      unlist(entries),
      "",
      unlist(sections)
    ),
    collapse = "\n\n"
  )
}

write_checked <- function(path, bytes, check) {
  if (check) {
    if (
      !file.exists(path) ||
        !identical(readBin(path, "raw", n = file.size(path)), bytes)
    ) {
      stop("Stale or missing notices: ", path)
    }
  } else {
    writeBin(bytes, path)
  }
}

generate <- function(check) {
  dir.create("notices", showWarnings = FALSE)
  pinned <- RcppTOML::parseTOML("rust-toolchain.toml")$toolchain$channel
  version <- strsplit(command_output("rustc", "--version"), " ", fixed = TRUE)[[
    1
  ]][2]
  if (version != pinned || pinned != "1.100.0") {
    stop(
      "Review the Rust standard library notices for toolchain ",
      pinned,
      "; active rustc is ",
      version
    )
  }
  std <- file.path(
    command_output("rustc", c("--print", "sysroot")),
    "share/doc/rust/COPYRIGHT-library.html"
  )
  # A minimal rustc installation may omit documentation. The checked-in copy
  # is byte-identical to this release's full Rust distribution.
  std_out <- "notices/rust-stdlib.html"
  if (!file.exists(std)) {
    std <- std_out
  }
  if (
    !file.exists(std) ||
      sha(std) !=
        "626556344c12679d7cab2c1aa2ce80de01937b62c2c88c62f0650a469be18799"
  ) {
    stop(
      "Expected Rust 1.100.0 share/doc/rust/COPYRIGHT-library.html from the official distribution"
    )
  }
  std_bytes <- readBin(std, "raw", n = file.size(std))
  windows <- "x86_64-pc-windows-msvc"
  cli <- package_set("cli", c("x86_64-unknown-linux-gnu", windows))
  host <- package_set("src-tauri", c("x86_64-pc-windows-gnu", windows))
  union <- c(cli, host[setdiff(names(host), names(cli))])
  if (
    !all(vapply(
      intersect(names(cli), names(host)),
      function(id) {
        identical(cli[[id]]$checksum, host[[id]]$checksum)
      },
      logical(1)
    ))
  ) {
    stop("The two Cargo locks disagree on a dependency checksum")
  }
  components <- lapply(union, notice_files)
  case_table <- readLines("scripts/windows-name-case.txt", encoding = "UTF-8")
  case_header <- sub("^# ?", "", case_table[startsWith(case_table, "#")])
  cli_text <- paste(
    render(cli, components, "CLI (Linux x64 and Windows x64)"),
    paste(case_header, collapse = "\n"),
    sep = "\n\n"
  )
  host_text <- paste(
    "shinytauri Windows host license",
    read_text("LICENSE"),
    render(host, components, "Windows x64 host (GNU and MSVC)"),
    sep = "\n\n"
  )
  installer <- read_text("scripts/notice-sources/installer-components.txt")
  write_checked(
    "notices/cli.txt",
    charToRaw(enc2utf8(paste0(cli_text, "\n"))),
    check
  )
  write_checked(
    "notices/host.txt",
    charToRaw(enc2utf8(paste0(host_text, "\n\n", installer))),
    check
  )
  write_checked(std_out, std_bytes, check)
  message(
    "Checked ",
    length(cli),
    " CLI and ",
    length(host),
    " host dependencies."
  )
}

if (sys.nframe() == 0L) {
  args <- commandArgs(trailingOnly = TRUE)
  if (length(args) && !identical(args, "--check")) {
    stop("Use no arguments or --check")
  }
  cargo_home <- path.expand(Sys.getenv("CARGO_HOME", "~/.cargo"))
  generate(identical(args, "--check"))
}
