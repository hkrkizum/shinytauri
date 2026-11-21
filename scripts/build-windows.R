# Run from the repository root with mise run build:windows.
platform <- "x86_64-w64-mingw32"
runtime_sha256 <- c(
  "4.6.2" = "d523d65c7788856a8d364c81118e759f0504cd467cd50e0bf9458f7d2809eb93"
)

download_zip <- function(url, file) {
  stopifnot(all(startsWith(url, "https://")))
  status <- utils::download.file(
    url,
    file,
    method = "libcurl",
    mode = "wb",
    quiet = TRUE
  )
  if (status != 0L || any(attr(status, "retvals") != 0L)) {
    stop("Could not download all requested archives.")
  }
}

unpack <- function(archive, dest, prefix) {
  entries <- utils::unzip(archive, list = TRUE)$Name
  if (
    !length(entries) ||
      !all(sub("/.*", "", entries) %in% prefix) ||
      any(grepl("[:\\\\]|//|(^|/)\\.{1,2}(/|$)", entries))
  ) {
    stop("Unexpected ZIP layout in ", archive)
  }
  withCallingHandlers(
    utils::unzip(archive, exdir = dest),
    warning = function(w) stop(conditionMessage(w), call. = FALSE)
  )
}

stage_runtime <- function(target, stage, payload) {
  version <- target$r_version
  if (!version %in% names(runtime_sha256)) {
    stop("Register the published portable R SHA-256 for R ", version)
  }
  runtime <- paste0("portable-r-", version, "-win-x64")
  archive <- file.path(stage, "R.zip")
  download_zip(
    sprintf(
      "https://github.com/portable-r/portable-r-windows/releases/download/v%s/%s.zip",
      version,
      runtime
    ),
    archive
  )
  if (tools::sha256sum(archive) != runtime_sha256[[version]]) {
    stop("SHA-256 mismatch for ", runtime)
  }
  unpack(archive, payload, runtime)
  stopifnot(
    file.rename(file.path(payload, runtime), file.path(payload, "R")),
    file.exists(file.path(payload, "R/bin/x64/Rterm.exe"))
  )
}

stage_quarto <- function(stage, payload) {
  version <- "1.11.9"
  digest <- "eda220a2f39c392f622049de013ddf7b4dfdf019ba877f251be14ce43e5734d1"
  archive <- file.path(stage, "quarto.zip")
  url <- sprintf(
    "https://github.com/quarto-dev/quarto-cli/releases/download/v%s/quarto-%s-win.zip",
    version,
    version
  )
  download_zip(url, archive)
  if (tools::sha256sum(archive) != digest) {
    stop("SHA-256 mismatch for Quarto ", version)
  }
  dest <- file.path(payload, "quarto")
  stopifnot(dir.create(dest))
  unpack(archive, dest, c("bin", "share"))
  stopifnot(file.exists(file.path(dest, "bin/quarto.exe")))
}

stage_packages <- function(target, stage, payload) {
  packages <- target$packages
  urls <- vapply(
    packages,
    function(p) {
      url <- p$sources[[1L]]
      if (
        !isTRUE(p$binary) || p$platform != platform || !endsWith(url, ".zip")
      ) {
        stop("No locked Windows binary for ", p$package)
      }
      url
    },
    character(1)
  )
  archives <- file.path(stage, paste0(seq_along(packages), ".zip"))
  download_zip(urls, archives)
  library_dir <- file.path(payload, "library")
  stopifnot(dir.create(library_dir))
  for (i in seq_along(packages)) {
    package <- packages[[i]]
    unpack(archives[[i]], library_dir, package$package)
    desc <- read.dcf(
      file.path(library_dir, package$package, "DESCRIPTION"),
      fields = c("Package", "Version")
    )
    stopifnot(
      desc[1L, "Package"] == package$package,
      desc[1L, "Version"] == package$version
    )
  }
  message(
    "Staged R ",
    target$r_version,
    " and ",
    length(packages),
    " Windows packages"
  )
}

read_bundle <- function(path) {
  spec <- RcppTOML::parseTOML(path)
  base <- dirname(path)
  for (key in c("app", "lock")) {
    value <- spec[[key]]
    if (!is.character(value) || length(value) != 1L || !nzchar(value)) {
      stop("Missing ", key, " in ", path)
    }
    spec[[key]] <- normalizePath(file.path(base, value), mustWork = TRUE)
  }
  if (!dir.exists(spec$app)) {
    stop("Expected app directory: ", spec$app)
  }
  files <- spec$files
  if (
    !is.character(files) ||
      !length(files) ||
      any(!nzchar(files) | grepl("[:\\\\]|^/|/$|//|(^|/)\\.{1,2}(/|$)", files))
  ) {
    stop("Expected relative app file or directory names in ", path)
  }
  spec$quarto <- spec$quarto %||% FALSE
  if (!identical(spec$quarto, TRUE) && !identical(spec$quarto, FALSE)) {
    stop("Expected a boolean for quarto in ", path)
  }
  spec
}

windows_target <- function(path) {
  lock <- RcppTOML::parseTOML(path)
  targets <- Filter(function(t) identical(t$platform, platform), lock$targets)
  if (length(targets) != 1L) {
    stop("Expected one Windows x64 target in ", path)
  }
  target <- targets[[1L]]
  target$packages <- Filter(function(p) "main" %in% p$groups, target$packages)
  packages <- vapply(target$packages, function(p) p$package, character(1))
  if (!"shiny" %in% packages) {
    stop("Missing Shiny runtime dependency in ", path)
  }
  target
}

stage_app <- function(spec, dest) {
  stopifnot(dir.create(dest, recursive = TRUE))
  for (relative in spec$files) {
    source <- file.path(spec$app, relative)
    if (!file.exists(source)) {
      stop("Missing app resource: ", source)
    }
    parent <- dirname(file.path(dest, relative))
    dir.create(parent, recursive = TRUE, showWarnings = FALSE)
    if (
      startsWith(
        normalizePath(dest, winslash = "/"),
        paste0(normalizePath(source, winslash = "/"), "/")
      )
    ) {
      stop("App resource contains the build directory: ", source)
    }
    if (!file.copy(source, parent, recursive = dir.exists(source))) {
      stop("Could not stage app resource: ", source)
    }
  }
}

build_windows <- function(profile) {
  options(timeout = max(120L, getOption("timeout")))
  spec <- read_bundle(profile)
  target <- windows_target(spec$lock)

  dir.create("src-tauri/target", recursive = TRUE, showWarnings = FALSE)
  stage <- tempfile("offline-", tmpdir = "src-tauri/target")
  stopifnot(dir.create(stage))
  on.exit(unlink(stage, recursive = TRUE), add = TRUE)
  payload <- file.path(stage, "payload")
  stage_app(spec, file.path(payload, "app"))
  stage_runtime(target, stage, payload)
  if (spec$quarto) {
    stage_quarto(stage, payload)
  }
  stage_packages(target, stage, payload)
  stopifnot(
    file.copy("scripts/serve-bundled.R", payload),
    file.copy("LICENSE", payload)
  )

  resources <- stats::setNames(
    list("offline"),
    file.path("target", basename(stage), "payload")
  )
  config <- file.path(stage, "bundle.json")
  settings <- list(bundle = list(resources = resources))
  settings$productName <- spec$productName
  settings$identifier <- spec$identifier
  jsonlite::write_json(
    settings,
    config,
    auto_unbox = TRUE
  )
  triple <- if (.Platform$OS.type == "windows") {
    "x86_64-pc-windows-msvc"
  } else {
    "x86_64-pc-windows-gnu"
  }
  # Tauri shares its NSIS work directory across all apps for this target.
  bundle_lock <- filelock::lock("src-tauri/target/build-windows.lock")
  on.exit(filelock::unlock(bundle_lock), add = TRUE)
  status <- system2(
    "cargo",
    c(
      "tauri",
      "build",
      "--target",
      triple,
      "--bundles",
      "nsis",
      "--config",
      shQuote(config),
      "--",
      "--locked"
    )
  )
  if (status != 0L) stop("cargo tauri build failed with status ", status)
}

if (sys.nframe() == 0L) {
  args <- commandArgs(trailingOnly = TRUE)
  if (length(args) > 1L) {
    stop("Expected at most one bundle TOML path")
  }
  build_windows(if (length(args)) args[[1L]] else "bundles/rhino.toml")
}
