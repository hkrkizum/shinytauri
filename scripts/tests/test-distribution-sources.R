# Verify the public source sidecar independently of the installer toolchain.
source("scripts/distribution-sources.R")
check_distribution_sources <- function() {
  fixture <- tempfile("source distribution ")
  dir.create(fixture)
  on.exit(unlink(fixture, recursive = TRUE))
  inputs <- file.path(fixture, "資料")
  project <- file.path(fixture, "Shiny app")
  output <- file.path(project, "dist")
  stage <- file.path(fixture, "stage")
  cache <- file.path(fixture, "cache")
  for (dir in c(inputs, project, stage, cache)) {
    dir.create(dir)
  }
  dir.create(file.path(inputs, ".cargo"))
  dir.create(file.path(inputs, "empty"))
  writeLines("build steps", file.path(inputs, "BUILD 日本語.md"))
  writeLines("project config", file.path(inputs, ".cargo", "config.toml"))
  writeLines("private Shiny app", file.path(project, "app.R"))
  profile <- file.path(project, "bundle.toml")
  spec <- list(app = project, files = "app.R", sourceBundle = "../資料")
  prepare <- function() {
    prepare_source_bundle(spec, profile, stage, output, cache)
  }
  pack <- prepare()
  stopifnot(
    valid_sha256(pack$sha256),
    identical(pack$file, paste0("shinytauri-sources-", pack$sha256, ".tar.gz")),
    all(
      c(".cargo/config.toml", "BUILD 日本語.md") %in%
        sub("^[.]/", "", utils::untar(pack$path, list = TRUE))
    ),
    !any(grepl("app.R", utils::untar(pack$path, list = TRUE), fixed = TRUE))
  )
  extracted <- file.path(fixture, "unpacked")
  dir.create(extracted)
  utils::untar(pack$path, exdir = extracted)
  stopifnot(
    dir.exists(file.path(extracted, "empty")),
    identical(
      readLines(file.path(extracted, "BUILD 日本語.md")),
      "build steps"
    ),
    identical(
      readLines(file.path(extracted, ".cargo/config.toml")),
      "project config"
    )
  )
  dir.create(output)
  publish_source_bundle(pack, output)
  path <- file.path(output, pack$file)
  stopifnot(identical(unname(tools::sha256sum(path)), pack$sha256))
  publish_source_bundle(pack, output)
  archive <- file.path(fixture, "提供.tar.gz")
  stopifnot(file.copy(path, archive))
  spec$sourceBundle <- archive
  again <- prepare()
  stopifnot(
    identical(again$sha256, pack$sha256),
    identical(again$file, pack$file)
  )
  writeLines("unrelated existing file", path)
  expect_bundle_error(publish_source_bundle(again, output), "existing")
  stopifnot(identical(readLines(path), "unrelated existing file"))
  spec$sourceBundle <- "../資料"
  dir.create(file.path(inputs, ".git"))
  expect_bundle_error(prepare(), ".git")
  unlink(file.path(inputs, ".git"), recursive = TRUE)
  writeLines("SECRET", file.path(inputs, ".env"))
  expect_bundle_error(prepare(), ".env")
  unlink(file.path(inputs, ".env"))
  if (.Platform$OS.type != "windows") {
    fifo <- file.path(inputs, "pipe")
    stopifnot(system2("mkfifo", shQuote(fifo)) == 0L)
    expect_bundle_error(prepare(), "regular file")
    unlink(fifo)
    stopifnot(file.symlink(
      file.path(project, "app.R"),
      file.path(inputs, "linked")
    ))
    expect_bundle_error(prepare(), "link")
    unlink(file.path(inputs, "linked"))
    stopifnot(file.symlink(inputs, file.path(fixture, "linked-inputs")))
    spec$sourceBundle <- "../linked-inputs"
    expect_bundle_error(prepare(), "link")
  }
  spec$sourceBundle <- "."
  expect_bundle_error(prepare(), "overlap")
  spec$sourceBundle <- "../資料"
  expect_bundle_error(
    prepare_source_bundle(
      spec,
      profile,
      stage,
      file.path(inputs, "dist"),
      cache
    ),
    "overlap"
  )
  spec$sourceBundle <- "dist/selected"
  dir.create(file.path(output, "selected"))
  expect_bundle_error(prepare(), "overlap")
  spec$sourceBundle <- "app.R"
  expect_bundle_error(prepare(), "overlap")
  spec$sourceBundle <- "../missing.zip"
  expect_bundle_error(prepare(), "sourceBundle")
  spec$sourceBundle <- NULL
  stopifnot(is.null(prepare()))
  stopifnot(identical(
    public_download_url(
      "https://user:private@example.org/pkg?token=hidden#tail"
    ),
    "https://example.org/pkg"
  ))
}
check_distribution_sources()


check_component_inventory <- function() {
  root <- tempfile("component inventory ")
  dir.create(root)
  on.exit(unlink(root, recursive = TRUE))
  engine <- file.path(root, "engine")
  payload <- file.path(root, "payload")
  notices <- file.path(engine, "notices")
  pkg <- file.path(payload, "library", "shiny")
  base_pkg <- file.path(payload, "R/library/base")
  for (dir in c(notices, pkg, base_pkg)) {
    dir.create(dir, recursive = TRUE)
  }
  for (name in c("rust-stdlib.html", "host.txt")) {
    writeLines(name, file.path(notices, name))
  }
  writeLines(
    c(
      "Package: shiny",
      "Version: 1.14.0",
      "Repository: RSPM",
      "License: MIT + file LICENSE"
    ),
    file.path(pkg, "DESCRIPTION")
  )
  writeLines(
    c("Package: base", "Version: 4.6.2"),
    file.path(base_pkg, "DESCRIPTION")
  )
  target <- list(
    r_version = "4.6.2",
    packages = list(list(package = "shiny", version = "1.14.0")),
    urls = "https://user:password@p3m.dev/cran/2026-10-01/bin/windows/contrib/4.6/shiny_1.14.0.zip?token=private"
  )
  writeLines("Legacy package binary fixture", file.path(root, "1.zip"))
  writeLines("Runtime fixture", file.path(root, "R.zip"))
  stage_distribution_notices(engine, target, root, payload, TRUE, NULL)
  inventory <- jsonlite::read_json(file.path(
    payload,
    "notices/components.json"
  ))
  shiny <- inventory$packages[[1L]]
  tools <- inventory$quarto$tools
  stopifnot(
    inventory$r$bundledPackages[[1L]]$version == "4.6.2",
    inventory$r$bundledPackages[[1L]]$name == "base",
    inventory$r$sha256 == unname(tools::sha256sum(file.path(root, "R.zip"))),
    shiny$sha256 == unname(tools::sha256sum(file.path(root, "1.zip"))),
    shiny$binaryUrl ==
      "https://p3m.dev/cran/2026-10-01/bin/windows/contrib/4.6/shiny_1.14.0.zip",
    shiny$license == "MIT + file LICENSE",
    shiny$sourceCandidates[[1L]] ==
      "https://p3m.dev/cran/2026-10-01/src/contrib/shiny_1.14.0.tar.gz",
    identical(
      shiny$sourceCandidates[[3L]],
      "https://cran.r-project.org/src/contrib/Archive/shiny/shiny_1.14.0.tar.gz"
    ),
    length(tools) == 7L,
    tools[[1L]]$name == "pandoc",
    tools[[1L]]$version == "3.10",
    endsWith(inventory$r$sourceArchive, "R-4.6.2.tar.gz"),
    inventory$r$thirdPartyNotices == "offline/R/doc/COPYRIGHTS"
  )
}
check_component_inventory()
