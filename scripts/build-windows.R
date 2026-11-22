# Invoked by the CLI in its private engine or by mise run build:windows.
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

archive_cache_dir <- function() {
  root <- Sys.getenv("SHINYTAURI_CACHE_DIR")
  if (!nzchar(root)) {
    root <- tools::R_user_dir("shinytauri", "cache")
  } else {
    absolute <- if (.Platform$OS.type == "windows") {
      grepl("^([A-Za-z]:/|//[^/]+/[^/]+)", chartr("\\", "/", root))
    } else {
      startsWith(root, "/")
    }
    if (!absolute) stop("SHINYTAURI_CACHE_DIR must be an absolute path: ", root)
  }
  file.path(root, "v1", "sha256")
}

lock_cache <- function(dir) {
  if (
    !dir.exists(dir) &&
      !dir.create(dir, recursive = TRUE, mode = "0700") &&
      !dir.exists(dir)
  ) {
    stop(
      "Cannot create archive cache: ",
      dir,
      ". Set SHINYTAURI_CACHE_DIR to a writable directory."
    )
  }
  filelock::lock(file.path(dir, "archives.lock"))
}

# All callers hold the cache lock. Publish complete, verified bytes on the
# cache's own filesystem, even when the private build directory is on another.
store_archives <- function(files, sums, dir) {
  part <- tempfile(".incoming-", tmpdir = dir)
  on.exit(unlink(part))
  for (i in seq_along(files)) {
    object <- file.path(dir, paste0(sums[[i]], ".zip"))
    if (file.exists(object) && isTRUE(tools::sha256sum(object) == sums[[i]])) {
      next
    }
    if (
      !file.copy(files[[i]], part, overwrite = TRUE) ||
        !isTRUE(tools::sha256sum(part) == sums[[i]])
    ) {
      stop("Could not copy verified archive to cache: ", object)
    }
    if (!file.rename(part, object)) {
      stop("Could not publish verified archive: ", object)
    }
  }
}

fetch_archives <- function(urls, files, pins = NULL) {
  if (is.null(pins)) {
    # A legacy URL alone does not establish the identity of bytes in a cache.
    download_zip(urls, files)
    return(invisible(NULL))
  }
  stopifnot(
    length(urls) == length(files),
    length(files) == length(pins),
    all(vapply(pins, valid_sha256, logical(1)))
  )
  dir <- archive_cache_dir()
  guard <- lock_cache(dir)
  on.exit(filelock::unlock(guard))
  # Verify the private copies that will actually be unpacked.
  hits <- vapply(
    seq_along(files),
    function(i) {
      object <- file.path(dir, paste0(pins[[i]], ".zip"))
      if (
        !file.exists(object) || !file.copy(object, files[[i]], overwrite = TRUE)
      ) {
        return(FALSE)
      }
      if (isTRUE(tools::sha256sum(files[[i]]) == pins[[i]])) {
        return(TRUE)
      }
      # An interrupted re-fetch must not leave this known-bad entry available.
      if (unlink(object) != 0L) {
        stop("Could not remove damaged cached archive: ", object)
      }
      FALSE
    },
    logical(1)
  )
  if (all(hits)) {
    return(invisible(NULL))
  }
  urls <- urls[!hits]
  files <- files[!hits]
  pins <- pins[!hits]
  download_zip(urls, files)
  sums <- tools::sha256sum(files)
  mismatches <- which(is.na(sums) | sums != pins)
  if (length(mismatches)) {
    i <- mismatches[[1L]]
    label <- if (is.null(names(urls))) basename(urls[[i]]) else names(urls)[[i]]
    stop("SHA-256 mismatch for ", label)
  }
  store_archives(files, pins, dir)
  invisible(NULL)
}

check_archive_layout <- function(archive, prefix) {
  entries <- utils::unzip(archive, list = TRUE)$Name
  if (
    !length(entries) ||
      !all(sub("/.*", "", entries) %in% prefix) ||
      any(grepl("[:\\\\]|//|(^|/)\\.{1,2}(/|$)", entries))
  ) {
    stop("Unexpected ZIP layout in ", archive)
  }
}

unpack <- function(archive, dest, prefix) {
  check_archive_layout(archive, prefix)
  withCallingHandlers(
    utils::unzip(archive, exdir = dest),
    warning = function(w) stop(conditionMessage(w), call. = FALSE)
  )
}

validate_r_version <- function(version) {
  component <- "(0|[1-9][0-9]*)"
  pattern <- paste0("\\A", component, "([.]", component, "){2}\\z")
  if (
    !is.character(version) ||
      length(version) != 1L ||
      is.na(version) ||
      !grepl(pattern, version, perl = TRUE)
  ) {
    stop("Expected an exact R version in major.minor.patch form")
  }
  version
}

portable_r_url <- function(version) {
  version <- validate_r_version(version)
  sprintf(
    "https://github.com/portable-r/portable-r-windows/releases/download/v%s/portable-r-%s-win-x64.zip",
    version,
    version
  )
}

valid_sha256 <- function(value) {
  is.character(value) &&
    length(value) == 1L &&
    !is.na(value) &&
    grepl("\\A[0-9a-fA-F]{64}\\z", value, perl = TRUE)
}

portable_r_digest <- function(version) {
  url <- portable_r_url(version)
  if (version %in% names(runtime_sha256)) {
    return(unname(runtime_sha256[[version]]))
  }
  file <- tempfile()
  on.exit(unlink(file))
  tryCatch(download_zip(paste0(url, ".sha256"), file), error = function(e) {
    stop(
      "Cannot obtain official portable R ",
      version,
      " x64 checksum: ",
      conditionMessage(e),
      call. = FALSE
    )
  })
  # Accept exactly one sha256sum record for this filename (LF or CRLF).
  # Validate its bytes before converting to text, including embedded NULs.
  name <- basename(url)
  max_size <- 66L + nchar(name) + 2L
  bytes <- readBin(file, "raw", n = max_size + 1L)
  if (length(bytes) > max_size || any(as.integer(bytes) %in% c(0L, 128:255))) {
    stop("Invalid official portable R checksum for ", name)
  }
  line <- sub("\\r?\\n\\z", "", rawToChar(bytes), perl = TRUE)
  digest <- substr(line, 1L, 64L)
  if (
    !valid_sha256(digest) ||
      !substr(line, 65L, 66L) %in% c("  ", " *") ||
      !identical(substring(line, 67L), name)
  ) {
    stop("Invalid official portable R checksum for ", name)
  }
  tolower(digest)
}

stage_runtime <- function(target, stage, payload) {
  version <- target$r_version
  url <- portable_r_url(version)
  runtime <- tools::file_path_sans_ext(basename(url))
  digest <- target$runtime_sha256 %||% portable_r_digest(version)
  archive <- file.path(stage, "R.zip")
  fetch_archives(url, archive, digest)
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
  fetch_archives(url, archive, digest)
  dest <- file.path(payload, "quarto")
  stopifnot(dir.create(dest))
  unpack(archive, dest, c("bin", "share"))
  stopifnot(file.exists(file.path(dest, "bin/quarto.exe")))
}

check_package <- function(desc, package) {
  fields <- read.dcf(desc, fields = c("Package", "Version"))
  if (
    nrow(fields) != 1L ||
      !identical(unname(fields[1L, ]), c(package$package, package$version))
  ) {
    stop(
      "Windows archive does not match locked package ",
      package$package,
      " ",
      package$version
    )
  }
}

pin_packages <- function(target, stage) {
  files <- file.path(stage, paste0(seq_along(target$urls), ".zip"))
  # Establish new pins from the current HTTPS responses at lock time.
  download_zip(target$urls, files)
  for (i in seq_along(files)) {
    pkg <- target$packages[[i]]
    check_archive_layout(files[[i]], pkg$package)
    check_package(unz(files[[i]], paste0(pkg$package, "/DESCRIPTION")), pkg)
  }
  sums <- unname(tools::sha256sum(files))
  stopifnot(all(vapply(sums, valid_sha256, logical(1))))
  dir <- archive_cache_dir()
  guard <- lock_cache(dir)
  on.exit(filelock::unlock(guard))
  store_archives(files, sums, dir)
  stats::setNames(sums, vapply(target$packages, function(p) p$package, ""))
}

stage_packages <- function(target, stage, payload) {
  packages <- target$packages
  archives <- file.path(stage, paste0(seq_along(packages), ".zip"))
  names <- vapply(packages, function(pkg) pkg$package, "")
  fetch_archives(
    stats::setNames(target$urls, names),
    archives,
    target$package_sha256
  )
  library_dir <- file.path(payload, "library")
  stopifnot(dir.create(library_dir))
  for (i in seq_along(packages)) {
    package <- packages[[i]]
    unpack(archives[[i]], library_dir, package$package)
    check_package(
      file.path(library_dir, package$package, "DESCRIPTION"),
      package
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

read_bundle <- function(path, for_lock = FALSE) {
  spec <- RcppTOML::parseTOML(path)
  base <- dirname(absolute_path(path))
  if (!is.null(spec$rVersion)) {
    validate_r_version(spec$rVersion)
  }
  spec$managed <- !is.null(spec$dependencyFile) ||
    !is.null(spec$packages) ||
    (!is.null(spec$rVersion) &&
      identical(spec$lock, ".shinytauri/windows.lock"))
  if (spec$managed && !identical(spec$lock, ".shinytauri/windows.lock")) {
    stop("For managed dependencies set lock = \".shinytauri/windows.lock\".")
  }
  for (key in intersect(c("app", "lock", "dependencyFile"), names(spec))) {
    value <- spec[[key]]
    if (!is.character(value) || length(value) != 1L || !nzchar(value)) {
      stop("Missing ", key, " in ", path)
    }
    path_value <- if (grepl("^(/|[A-Za-z]:[/\\\\])", value)) {
      value
    } else {
      file.path(base, value)
    }
    if (key == "lock" && spec$managed) {
      spec$lock <- path_value
      if (!for_lock && !file.exists(path_value)) {
        stop("Missing Windows lock: run shinytauri lock for this project.")
      }
      next
    }
    if (key == "lock" && !file.exists(path_value)) {
      stop(
        "Missing Windows runtime lock: ",
        path_value,
        ". Run 'rig proj lock' in ",
        dirname(path_value),
        " after declaring runtime dependencies."
      )
    }
    spec[[key]] <- normalizePath(path_value, winslash = "/", mustWork = TRUE)
    if (
      key == "lock" &&
        identical(value, ".shinytauri/rproj.lock") &&
        is.null(spec$checkMinimal)
    ) {
      # The 0.1.0 CLI copied its built-in lock verbatim (LF or CRLF checkout).
      # A separately resolved application lock still owns its dependencies.
      spec$checkMinimal <- unname(tools::sha256sum(path_value)) %in%
        c(
          "85384c4ddaee1162eb7749df066c9bd679d60b012a7d74a3c54df90701023783",
          "19aa92b54da9bf47aa52d889a1377e8e8db06e9af754934812c1e9024b4d63a3"
        )
    }
  }
  if (is.null(spec$lock)) {
    stop("Missing lock in ", path)
  }
  if (is.null(spec$app) || !dir.exists(spec$app)) {
    stop("Expected app directory: ", spec$app)
  }
  for (key in c("files", "emptyDirs")) {
    entries <- spec[[key]]
    if (key == "emptyDirs" && is.null(entries)) {
      next
    }
    if (
      !is.character(entries) ||
        !length(entries) ||
        any(
          !nzchar(entries) |
            grepl("[:\\\\]|^/|/$|//|(^|/)\\.{1,2}(/|$)", entries)
        )
    ) {
      stop("Expected relative app paths for ", key, " in ", path)
    }
  }
  spec$quarto <- spec$quarto %||% FALSE
  if (!identical(spec$quarto, TRUE) && !identical(spec$quarto, FALSE)) {
    stop("Expected a boolean for quarto in ", path)
  }
  for (key in intersect(
    c("productName", "identifier", "version", "title"),
    names(spec)
  )) {
    value <- spec[[key]]
    if (!is.character(value) || length(value) != 1L || !nzchar(trimws(value))) {
      stop("Expected a nonempty string for ", key)
    }
  }
  if (
    !is.null(spec$productName) &&
      (grepl('[<>:"/\\\\|?*[:cntrl:]]|[. ]$', spec$productName) ||
        grepl(
          "^(con|prn|aux|nul|com[1-9]|lpt[1-9])([.]|$)",
          spec$productName,
          ignore.case = TRUE
        ))
  ) {
    stop("Invalid Windows productName: ", spec$productName)
  }
  if (
    !is.null(spec$identifier) &&
      !grepl("^[A-Za-z0-9-]+([.][A-Za-z0-9-]+)*$", spec$identifier)
  ) {
    stop("Invalid Tauri identifier: ", spec$identifier)
  }
  if (
    !is.null(spec$version) &&
      !grepl(
        "^(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)$",
        spec$version
      )
  ) {
    stop("Expected a numeric major.minor.patch version: ", spec$version)
  }
  spec
}

windows_target <- function(path, r_version = NULL) {
  lock <- RcppTOML::parseTOML(path)
  targets <- Filter(function(t) identical(t$platform, platform), lock$targets)
  windows_count <- length(targets)
  if (!is.null(r_version)) {
    validate_r_version(r_version)
    targets <- Filter(function(t) identical(t$r_version, r_version), targets)
  }
  if (length(targets) != 1L) {
    hint <- if (
      identical(basename(dirname(path)), ".shinytauri") &&
        identical(basename(path), "rproj.lock")
    ) {
      ' To change the bundled minimum, set lock = ".shinytauri/windows.lock" and run shinytauri lock.'
    } else if (is.null(r_version) && length(targets) > 1L) {
      paste0(
        " Set rVersion to one of: ",
        paste(
          unique(vapply(targets, function(t) t$r_version, "")),
          collapse = ", "
        ),
        "."
      )
    }
    stop(
      "Expected one Windows x64 target",
      if (!is.null(r_version)) paste0(" for R ", r_version),
      " in ",
      path,
      ".",
      hint
    )
  }
  target <- targets[[1L]]
  version <- validate_r_version(target$r_version)
  pin <- lock$shinytauri$runtime_sha256
  if (!is.null(pin)) {
    if (!valid_sha256(pin)) {
      stop("Invalid portable R SHA-256 in ", path)
    }
    pin <- tolower(pin)
  }
  if (version %in% names(runtime_sha256)) {
    trusted <- unname(runtime_sha256[[version]])
    if (!is.null(pin) && !identical(pin, trusted)) {
      stop("Lock disagrees with built-in SHA-256 for R ", version)
    }
    pin <- trusted
  }
  target$runtime_sha256 <- pin
  target$packages <- Filter(function(p) "main" %in% p$groups, target$packages)
  packages <- vapply(target$packages, function(p) p$package, character(1))
  pins <- lock$shinytauri[["package_sha256"]]
  format <- lock$shinytauri[["archive_format"]]
  if (!is.null(format) || !is.null(pins)) {
    if (
      windows_count != 1L ||
        !identical(format, 1L) ||
        !is.list(pins) ||
        anyDuplicated(packages) ||
        !setequal(names(pins), packages) ||
        !all(vapply(pins, valid_sha256, logical(1)))
    ) {
      stop("Invalid or incomplete Windows package SHA-256 table in ", path)
    }
    target$package_sha256 <- unname(tolower(vapply(
      pins[packages],
      identity,
      ""
    )))
  }
  if (!"shiny" %in% packages) {
    stop("Missing Shiny runtime dependency in ", path)
  }
  target$urls <- vapply(
    target$packages,
    function(p) {
      url <- p$sources[[1L]]
      if (
        !isTRUE(p$binary) ||
          !identical(p$platform, platform) ||
          !is.character(url) ||
          length(url) != 1L ||
          !startsWith(url, "https://") ||
          !endsWith(url, ".zip")
      ) {
        stop("No locked Windows binary for ", p$package)
      }
      # Standard CRAN binary URLs identify the R major.minor ABI.
      abi <- sub("[.][0-9]+$", "", version)
      path <- sub("[?#].*$", "", url)
      contrib <- strsplit(path, "/bin/windows/contrib/", fixed = TRUE)[[1L]]
      if (
        length(contrib) > 1L && !startsWith(contrib[[2L]], paste0(abi, "/"))
      ) {
        stop("Windows binary for ", p$package, " does not target R ", abi)
      }
      url
    },
    character(1)
  )
  target
}

# Canonicalize the existing ancestor as well when the output doesn't exist yet.
absolute_path <- function(path) {
  if (file.exists(path)) {
    return(normalizePath(path, winslash = "/", mustWork = TRUE))
  }
  parent <- dirname(path)
  if (parent == path) {
    stop("Path does not exist: ", path)
  }
  file.path(absolute_path(parent), basename(path))
}

within_path <- function(path, directory) {
  if (.Platform$OS.type == "windows") {
    path <- tolower(path)
    directory <- tolower(directory)
  }
  path == directory || startsWith(path, paste0(directory, "/"))
}

copy_resource <- function(source, destination, directory_only = FALSE) {
  link <- Sys.readlink(source)
  resolved <- absolute_path(source)
  if (
    (!is.na(link) && nzchar(link)) ||
      !within_path(resolved, source) ||
      !within_path(source, resolved)
  ) {
    stop("App resource contains a link: ", source)
  }
  if (!file.exists(source)) {
    stop("Missing app resource: ", source)
  }
  if (dir.exists(source)) {
    if (
      !dir.exists(destination) && !dir.create(destination, recursive = TRUE)
    ) {
      stop("Could not stage app directory: ", source)
    }
    if (directory_only) {
      return(invisible(NULL))
    }
    if (file.access(source, 4L) != 0L) {
      stop("Unreadable app directory: ", source)
    }
    children <- list.files(source, all.files = TRUE, no.. = TRUE)
    for (child in children) {
      copy_resource(
        file.path(source, child),
        file.path(destination, child)
      )
    }
  } else {
    dir.create(dirname(destination), recursive = TRUE, showWarnings = FALSE)
    if (!file.copy(source, destination, overwrite = TRUE)) {
      stop("Could not stage app resource: ", source)
    }
  }
}

stage_app <- function(spec, dest, output = NULL) {
  stopifnot(dir.create(dest, recursive = TRUE))
  dest <- absolute_path(dest)
  app <- absolute_path(spec$app)
  for (relative in unique(c(spec$files, spec$emptyDirs))) {
    source <- file.path(app, relative)
    resolved <- absolute_path(source)
    if (within_path(dest, resolved)) {
      stop("App resource contains the build directory: ", source)
    }
    if (!is.null(output) && within_path(output, resolved)) {
      stop("App resource overlaps the output directory: ", source)
    }
    if (relative %in% spec$emptyDirs && !dir.exists(source)) {
      stop("Expected existing app directory: ", source)
    }
    copy_resource(
      source,
      file.path(dest, relative),
      directory_only = !relative %in% spec$files
    )
  }
  # NSIS installs files. An empty staged leaf needs a file to carry its path.
  for (directory in list.dirs(dest)) {
    if (!length(list.files(directory, all.files = TRUE, no.. = TRUE))) {
      stopifnot(file.create(file.path(directory, ".gitkeep")))
    }
  }
  # shinyAppDir accepts app.R or server.R and looks them up case-insensitively.
  entry <- list.files(
    dest,
    pattern = "^(app|server)[.]r$",
    full.names = TRUE,
    ignore.case = TRUE
  )
  if (!any(utils::file_test("-f", entry))) {
    stop("Missing Shiny entry point in files: include app.R or server.R")
  }
}

run_tauri <- function(engine, config, triple, target_dir) {
  old <- setwd(file.path(engine, "src-tauri"))
  on.exit(setwd(old))
  # cargo metadata and the Tauri bundler must agree on the artifact directory.
  previous <- Sys.getenv("CARGO_TARGET_DIR", unset = NA_character_)
  Sys.setenv(CARGO_TARGET_DIR = target_dir)
  on.exit(
    if (is.na(previous)) {
      Sys.unsetenv("CARGO_TARGET_DIR")
    } else {
      Sys.setenv(CARGO_TARGET_DIR = previous)
    },
    add = TRUE
  )
  # Cargo searches CARGO_HOME/bin before PATH for external subcommands. The
  # Nix shell supplies the exact cross-build CLI even if users installed another.
  tauri <- Sys.getenv("SHINYTAURI_CARGO_TAURI")
  status <- system2(
    if (nzchar(tauri)) tauri else "cargo",
    c(
      if (!nzchar(tauri)) "tauri",
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

publish_installer <- function(target_dir, triple, output, sources) {
  nsis <- file.path(target_dir, triple, "release/bundle/nsis")
  installer <- list.files(nsis, pattern = "[.]exe$", full.names = TRUE)
  if (length(installer) != 1L || file.info(installer)$size == 0) {
    stop("Expected one new NSIS installer in ", nsis)
  }
  if (
    !dir.exists(output) &&
      !dir.create(output, recursive = TRUE) &&
      !dir.exists(output)
  ) {
    stop("Could not create output directory: ", output)
  }
  # Copy across volumes, then replace in the same directory. A failed build
  # or interrupted copy leaves the previous published installer available.
  part <- tempfile(".shinytauri-", tmpdir = output)
  on.exit(unlink(part))
  file <- file.path(output, basename(installer))
  if (
    file.exists(file) &&
      any(vapply(
        sources,
        function(source) within_path(file, source),
        logical(1)
      ))
  ) {
    stop("Installer would overwrite an app resource: ", file)
  }
  if (!file.copy(installer, part) || !file.rename(part, file)) {
    stop("Could not publish installer to ", file)
  }
  message("Installer: ", file)
  invisible(file)
}

build_windows <- function(profile, output = NULL, engine = getwd()) {
  options(timeout = max(120L, getOption("timeout")))
  spec <- read_bundle(profile)
  engine <- absolute_path(engine)
  if (spec$managed || isTRUE(spec$checkMinimal)) {
    source(file.path(engine, "scripts/runtime-deps.R"), local = TRUE)
  }
  target <- windows_target(spec$lock, if (!spec$managed) spec$rVersion)
  if (spec$managed) {
    verify_runtime_lock(spec, target)
  }
  if (!is.null(output)) {
    output <- absolute_path(output)
  }

  # Never copy cache objects into the app when a cache override lies inside
  # a recursively selected resource (including after an earlier lock command).
  cache <- absolute_path(archive_cache_dir())
  if (
    any(vapply(
      file.path(spec$app, spec$files),
      function(source) within_path(cache, source),
      FALSE
    ))
  ) {
    stop(
      "The archive cache overlaps an app resource. Set SHINYTAURI_CACHE_DIR outside files."
    )
  }

  tauri <- file.path(engine, "src-tauri")
  target_dir <- file.path(tauri, "target")
  dir.create(target_dir, recursive = TRUE, showWarnings = FALSE)
  stage <- tempfile("offline-", tmpdir = target_dir)
  stopifnot(dir.create(stage))
  on.exit(unlink(stage, recursive = TRUE), add = TRUE)
  payload <- file.path(stage, "payload")
  stage_app(spec, file.path(payload, "app"), output)
  if (isTRUE(spec$checkMinimal)) {
    check_minimal_imports(file.path(payload, "app"), target)
  }
  stage_runtime(target, stage, payload)
  if (spec$quarto) {
    stage_quarto(stage, payload)
  }
  stage_packages(target, stage, payload)
  stopifnot(
    file.copy(file.path(engine, "scripts/serve-bundled.R"), payload),
    file.copy(file.path(engine, "LICENSE"), payload)
  )

  resources <- stats::setNames(
    list("offline"),
    file.path("target", basename(stage), "payload")
  )
  config <- file.path(stage, "bundle.json")
  settings <- list(bundle = list(resources = resources))
  settings$productName <- spec$productName
  settings$identifier <- spec$identifier
  settings$version <- spec$version
  title <- spec$title %||% spec$productName
  if (!is.null(title)) {
    # Tauri replaces arrays when applying a JSON merge, so retain the complete
    # window configuration when overriding just the title of the main window.
    base <- jsonlite::read_json(file.path(tauri, "tauri.conf.json"))
    windows <- base$app$windows
    windows[[1L]]$title <- title
    settings$app <- list(windows = windows)
  }
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
  if (!is.null(output)) {
    # Keep this build's bundle separate, even when earlier installers exist.
    cargo_dir <- file.path(stage, "cargo")
    run_tauri(engine, config, triple, cargo_dir)
    return(publish_installer(
      cargo_dir,
      triple,
      output,
      file.path(spec$app, spec$files)
    ))
  }
  # Legacy mise builds share the target tree and leave the installer there.
  bundle_lock <- filelock::lock(file.path(target_dir, "build-windows.lock"))
  on.exit(filelock::unlock(bundle_lock), add = TRUE)
  run_tauri(engine, config, triple, target_dir)
}

if (sys.nframe() == 0L) {
  args <- commandArgs(trailingOnly = TRUE)
  if (length(args) > 2L) {
    stop("Expected [bundle TOML] [output directory]")
  }
  script <- sub(
    "^--file=",
    "",
    grep("^--file=", commandArgs(), value = TRUE)[1L]
  )
  # Rscript encodes spaces in its --file argument on Unix.
  script <- gsub("~+~", " ", script, fixed = TRUE)
  engine <- dirname(dirname(normalizePath(script, mustWork = TRUE)))
  if (length(args) == 2L && identical(args[[1L]], "--lock")) {
    source(file.path(engine, "scripts/runtime-deps.R"))
    lock_windows(read_bundle(args[[2L]], for_lock = TRUE))
  } else {
    build_windows(
      if (length(args)) args[[1L]] else file.path(engine, "bundles/rhino.toml"),
      if (length(args) == 2L) args[[2L]] else NULL,
      engine
    )
  }
}
