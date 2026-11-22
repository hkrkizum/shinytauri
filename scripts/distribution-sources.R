# Package only materials the distributor explicitly prepared for publication.
# An upstream catalogue cannot establish which sources reproduce modified binaries.
source_path_without_links <- function(path) {
  parent <- dirname(path)
  if (parent != path) {
    source_path_without_links(parent)
  }
  if (utils::file_test("-L", path)) {
    stop("sourceBundle contains a link: ", path)
  }
  if (!file.exists(path)) {
    stop("Missing sourceBundle path: ", path)
  }
  if (parent != path && !basename(path) %in% c(".", "..")) {
    expected <- file.path(sub("/$", "", absolute_path(parent)), basename(path))
    actual <- absolute_path(path)
    # normalizePath also resolves Windows junctions, which Sys.readlink may miss.
    if (!within_path(expected, actual) || !within_path(actual, expected)) {
      stop("sourceBundle contains a link: ", path)
    }
  }
  invisible(NULL)
}

regular_source_file <- function(path) {
  # R's file_test("-f") on Unix also accepts FIFOs and device nodes.
  if (.Platform$OS.type == "unix") {
    return(system2("test", c("-f", shQuote(path))) == 0L)
  }
  utils::file_test("-f", path)
}

copy_source_material <- function(path, dest) {
  if (
    utils::file_test("-L", path) ||
      !within_path(absolute_path(path), path) ||
      !within_path(path, absolute_path(path))
  ) {
    stop("sourceBundle contains a link: ", path)
  }
  name <- tolower(basename(path))
  if (
    name %in%
      c(
        ".git",
        ".hg",
        ".svn",
        ".ssh",
        ".gnupg",
        ".aws",
        ".netrc",
        ".renviron",
        ".rhistory",
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        ".env"
      ) ||
      startsWith(name, ".env.") ||
      tools::file_ext(name) %in% c("p12", "pfx")
  ) {
    stop("Remove private or VCS material from the public sourceBundle: ", path)
  }
  if (dir.exists(path)) {
    if (
      file.access(path, 4L) != 0L ||
        (.Platform$OS.type == "unix" && file.access(path, 1L) != 0L) ||
        !dir.create(dest)
    ) {
      stop("Cannot copy sourceBundle directory: ", path)
    }
    entries <- withCallingHandlers(
      list.files(path, all.files = TRUE, no.. = TRUE),
      warning = function(w) {
        stop("Cannot enumerate sourceBundle: ", conditionMessage(w))
      }
    )
    for (entry in entries) {
      copy_source_material(file.path(path, entry), file.path(dest, entry))
    }
    Sys.setFileTime(dest, file.mtime(path))
  } else if (
    !regular_source_file(path) || !file.copy(path, dest, copy.date = TRUE)
  ) {
    stop("Expected a readable regular file in sourceBundle: ", path)
  }
}

prepare_source_bundle <- function(spec, profile, stage, output, cache) {
  value <- spec[["sourceBundle"]]
  if (is.null(value)) {
    return(NULL)
  }
  if (
    !is.character(value) ||
      length(value) != 1L ||
      is.na(value) ||
      !nzchar(trimws(value))
  ) {
    stop(
      "sourceBundle must be a path to a prepared directory or source archive."
    )
  }
  path <- if (grepl("^(/|[A-Za-z]:[/\\\\])", value)) {
    value
  } else {
    file.path(dirname(absolute_path(profile)), value)
  }
  source_path_without_links(path)
  path <- absolute_path(path)
  app <- absolute_path(spec$app)
  if (!is.null(output)) {
    output <- absolute_path(output)
  }
  selected <- c(spec$files, spec$emptyDirs)
  app_inputs <- if (length(selected)) file.path(app, selected)
  restricted <- c(
    stage,
    cache,
    app_inputs,
    spec[["icon"]],
    spec[["licenseFile"]]
  )
  # An output parent such as the project itself is valid. Never archive an
  # output directory or place a source archive/installer over selected input.
  overlaps <- function(other) {
    other <- absolute_path(other)
    within_path(other, path) || within_path(path, other)
  }
  if (
    within_path(app, path) ||
      (!is.null(output) &&
        (within_path(output, path) ||
          (within_path(path, output) && !within_path(app, output)))) ||
      any(vapply(restricted, overlaps, logical(1)))
  ) {
    stop(
      "sourceBundle overlaps the app, selected resources, staging, cache or output: ",
      path
    )
  }
  part <- tempfile("sources-", tmpdir = stage)
  if (dir.exists(path)) {
    snapshot <- tempfile("source-tree-", tmpdir = stage)
    copy_source_material(path, snapshot)
    if (!length(list.files(snapshot, all.files = TRUE, no.. = TRUE))) {
      stop("Empty sourceBundle directory: ", path)
    }
    old <- setwd(snapshot)
    on.exit(setwd(old), add = TRUE)
    utils::tar(
      part,
      files = ".",
      compression = "gzip",
      tar = "internal",
      extra_flags = character()
    )
    extension <- ".tar.gz"
  } else {
    pattern <- "[.](tar[.](gz|xz|bz2|zst)|tar|tgz|txz|tbz2|zip|7z)$"
    extension <- regmatches(tolower(path), regexpr(pattern, tolower(path)))
    if (
      length(extension) != 1L ||
        !regular_source_file(path) ||
        file.size(path) == 0L ||
        !file.copy(path, part)
    ) {
      stop(
        "Expected a nonempty sourceBundle archive (.tar.gz, .tar.xz, .zip, etc.): ",
        path
      )
    }
  }
  digest <- unname(tools::sha256sum(part))
  stopifnot(valid_sha256(digest))
  list(
    path = part,
    file = paste0("shinytauri-sources-", digest, extension),
    sha256 = digest
  )
}

publish_source_bundle <- function(bundle, output) {
  if (is.null(bundle)) {
    return(invisible(NULL))
  }
  destination <- file.path(output, bundle$file)
  matches <- function() {
    !utils::file_test("-L", destination) &&
      regular_source_file(destination) &&
      isTRUE(tools::sha256sum(destination) == bundle$sha256)
  }
  if (file.exists(destination) || utils::file_test("-L", destination)) {
    if (!matches()) {
      stop("Refusing to replace an existing sourceBundle: ", destination)
    }
    return(invisible(destination))
  }
  part <- file.path(output, basename(tempfile(".shinytauri-source-")))
  on.exit(unlink(part))
  if (
    !file.copy(bundle$path, part) ||
      !isTRUE(tools::sha256sum(part) == bundle$sha256)
  ) {
    stop("Could not copy verified sourceBundle to ", output)
  }
  if (!file.rename(part, destination) && !matches()) {
    stop("Could not publish sourceBundle: ", destination)
  }
  invisible(destination)
}

public_download_url <- function(url) {
  sub("^(https?://)[^/]*@", "\\1", sub("[?#].*$", "", url))
}

quarto_components <- function() {
  tools <- list(
    c("pandoc", "3.10", "jgm/pandoc", "3.10"),
    c("deno", "2.9.8", "denoland/deno", "v2.9.8"),
    c(
      "deno_dom",
      "0.1.41-alpha-artifacts",
      "b-fuze/deno-dom",
      "v0.1.41-alpha-artifacts"
    ),
    c("dart-sass", "1.101.0", "sass/dart-sass", "1.101.0"),
    c("esbuild", "0.28.1", "evanw/esbuild", "v0.28.1"),
    c("typst", "0.15.1", "typst/typst", "v0.15.1"),
    c("typst-gather", "0.2.3", "quarto-dev/typst-gather", "v0.2.3")
  )
  stopifnot(identical(quarto_version, "1.11.9"))
  repo <- "https://github.com/quarto-dev/quarto-cli"
  list(
    version = quarto_version,
    binaryUrl = quarto_binary_url,
    sha256 = quarto_sha256,
    upstream = paste0(repo, "/tree/v", quarto_version),
    buildConfiguration = paste0(
      repo,
      "/blob/v",
      quarto_version,
      "/configuration"
    ),
    tools = lapply(tools, function(row) {
      list(
        name = row[[1]],
        version = row[[2]],
        upstream = paste(
          "https://github.com",
          row[[3]],
          "tree",
          row[[4]],
          sep = "/"
        )
      )
    })
  )
}

stage_distribution_notices <- function(
  engine,
  target,
  stage,
  payload,
  quarto,
  bundle
) {
  notices <- file.path(payload, "notices")
  stopifnot(dir.create(notices))
  for (file in c("host.txt", "rust-stdlib.html")) {
    stopifnot(file.copy(file.path(engine, "notices", file), notices))
  }
  version <- target$r_version
  descriptions <- Sys.glob(file.path(payload, "R/library/*/DESCRIPTION"))
  bundled <- unname(lapply(descriptions, function(desc) {
    dcf <- read.dcf(desc, fields = c("Package", "Version"))
    list(name = unname(dcf[1, "Package"]), version = unname(dcf[1, "Version"]))
  }))
  digest <- function(pin, file) {
    if (!is.null(pin)) {
      return(unname(pin))
    }
    if (file.exists(file)) unname(tools::sha256sum(file))
  }
  packages <- lapply(seq_along(target$packages), function(i) {
    pkg <- target$packages[[i]]
    item <- list(
      name = pkg$package,
      version = pkg$version,
      binaryUrl = public_download_url(target$urls[[i]]),
      sha256 = digest(
        target$package_sha256[i],
        file.path(stage, paste0(i, ".zip"))
      )
    )
    desc <- file.path(payload, "library", pkg$package, "DESCRIPTION")
    if (file.exists(desc)) {
      meta <- read.dcf(desc, fields = c("Repository", "License"))
      item$license <- unname(meta[1, "License"])
      from_cran <- identical(unname(meta[1, "Repository"]), "CRAN") ||
        grepl(
          "^https://(p3m[.]dev|packagemanager[.]posit[.]co)/cran/",
          item$binaryUrl
        )
      if (from_cran) {
        archive <- paste0(pkg$package, "_", pkg$version, ".tar.gz")
        item$sourceCandidates <- list(
          paste0("https://cran.r-project.org/src/contrib/", archive),
          paste0(
            "https://cran.r-project.org/src/contrib/Archive/",
            pkg$package,
            "/",
            archive
          )
        )
        snapshot <- "^https://(p3m[.]dev|packagemanager[.]posit[.]co)/cran/[0-9]{4}-[0-9]{2}-[0-9]{2}/bin/windows/contrib/"
        if (grepl(snapshot, item$binaryUrl)) {
          item$sourceCandidates <- c(
            list(paste0(
              sub("/bin/windows/contrib/.*$", "/src/contrib/", item$binaryUrl),
              archive
            )),
            item$sourceCandidates
          )
        }
      }
    }
    item
  })
  manifest <- list(
    schemaVersion = 1L,
    sourceBundle = if (!is.null(bundle)) bundle[c("file", "sha256")],
    sourceGuidance = paste(
      "The distributor must supply the complete corresponding source where the licenses require it.",
      "Include the actual sources, changes, bundled dependencies and build/install scripts for the distributed versions.",
      "This inventory and upstream links are starting points; additional or modified materials may be required."
    ),
    r = list(
      version = version,
      binaryUrl = portable_r_url(version),
      sha256 = digest(target$runtime_sha256, file.path(stage, "R.zip")),
      sourceArchive = sprintf(
        "https://cran.r-project.org/src/base/R-%s/R-%s.tar.gz",
        sub("[.].*$", "", version),
        version
      ),
      portableBuild = paste0(
        "https://github.com/portable-r/portable-r-windows/tree/v",
        version
      ),
      windowsBuildTools = "https://cran.r-project.org/bin/windows/Rtools/",
      thirdPartyNotices = "offline/R/doc/COPYRIGHTS",
      sourceGuidance = "See R's doc/COPYRIGHTS for the Windows build's Rtools, Tcl/Tk and bitmap library sources, versions and notices. Preserve packaging scripts and any changes.",
      bundledPackages = bundled
    ),
    packages = packages,
    packageSourceGuidance = paste(
      "Use each installed offline/library/<package>/DESCRIPTION for provenance.",
      "For CRAN match the exact release in src/contrib or https://cran.r-project.org/src/contrib/Archive/.",
      "For other repositories and patched binaries obtain the matching sources and build inputs from their distributor."
    ),
    quarto = if (quarto) quarto_components()
  )
  jsonlite::write_json(
    manifest,
    file.path(notices, "components.json"),
    auto_unbox = TRUE,
    pretty = TRUE,
    null = "null",
    na = "null"
  )
}
