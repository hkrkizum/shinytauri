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
  packages <- Filter(function(p) "main" %in% p$groups, target$packages)
  stopifnot(length(packages) > 0L)
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

build_windows <- function() {
  options(timeout = max(120L, getOption("timeout")))
  lock <- RcppTOML::parseTOML("rhino/rproj.lock")
  targets <- Filter(function(t) identical(t$platform, platform), lock$targets)
  stopifnot(length(targets) == 1L)
  target <- targets[[1L]]

  dir.create("src-tauri/target", recursive = TRUE, showWarnings = FALSE)
  stage <- tempfile("offline-", tmpdir = "src-tauri/target")
  stopifnot(dir.create(stage))
  on.exit(unlink(stage, recursive = TRUE), add = TRUE)
  payload <- file.path(stage, "payload")
  app <- file.path(payload, "rhino")
  stopifnot(dir.create(app, recursive = TRUE))

  stage_runtime(target, stage, payload)
  stage_quarto(stage, payload)
  stage_packages(target, stage, payload)
  stopifnot(
    file.copy("rhino/app", app, recursive = TRUE),
    all(file.copy(
      file.path("rhino", c("app.R", "rhino.yml", "config.yml")),
      app
    )),
    file.copy("scripts/serve-bundled.R", app),
    file.copy("LICENSE", payload)
  )

  resources <- stats::setNames(
    list("offline"),
    file.path("target", basename(stage), "payload")
  )
  config <- file.path(stage, "bundle.json")
  jsonlite::write_json(
    list(bundle = list(resources = resources)),
    config,
    auto_unbox = TRUE
  )
  triple <- if (.Platform$OS.type == "windows") {
    "x86_64-pc-windows-msvc"
  } else {
    "x86_64-pc-windows-gnu"
  }
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

build_windows()
