source("scripts/build-windows.R")

check_bundle <- function(profile, included, excluded, quarto) {
  spec <- read_bundle(profile)
  target <- windows_target(spec$lock)
  packages <- vapply(target$packages, function(p) p$package, character(1))
  stopifnot(
    all(vapply(
      target$packages,
      function(p) isTRUE(p$binary) && endsWith(p$sources[[1L]], ".zip"),
      logical(1)
    )),
    "shiny" %in% packages,
    !any(c("RcppTOML", "shinytest2") %in% packages),
    spec$quarto == quarto
  )
  dest <- tempfile()
  on.exit(unlink(dest, recursive = TRUE))
  stage_app(spec, dest)
  stopifnot(
    all(file.exists(file.path(dest, included))),
    !any(file.exists(file.path(dest, excluded)))
  )
}

check_bundle(
  "bundles/rhino.toml",
  c(
    "app.R",
    "app/main.R",
    "app/report/dashboard.qmd",
    "config.yml",
    "rhino.yml"
  ),
  c(".rvenv", ".Renviron", "tests", "serve-bundled.R"),
  TRUE
)
check_bundle(
  "bundles/minimal/bundle.toml",
  c("app.R", "www/style.css"),
  c("rproj.lock", "rhino.yml", "serve-bundled.R"),
  FALSE
)

# A build-host lock is valid TOML but cannot run the packaged Shiny app.
stopifnot(inherits(
  try(windows_target("rproj.lock"), silent = TRUE),
  "try-error"
))

# The source tree can have a layout that neither shipped example uses.
check_custom_layout <- function() {
  root <- tempfile("bundle with spaces ")
  on.exit(unlink(root, recursive = TRUE))
  app <- file.path(root, "source")
  stopifnot(dir.create(file.path(app, "my data"), recursive = TRUE))
  stopifnot(file.create(file.path(app, c("ui.R", "server.R", "my data/a.csv"))))
  profile <- file.path(root, "bundle.toml")
  lock <- file.path(root, "rproj.lock")
  stopifnot(file.copy("bundles/minimal/rproj.lock", lock))
  writeLines(
    c(
      'app = "source"',
      'lock = "rproj.lock"',
      'files = ["ui.R", "server.R", "my data/a.csv"]'
    ),
    profile
  )
  dest <- file.path(root, "stage")
  stage_app(read_bundle(profile), dest)
  stopifnot(file.exists(file.path(dest, "my data/a.csv")))
  writeLines(
    c('app = "source"', 'lock = "rproj.lock"', 'files = ["../rproj.lock"]'),
    profile
  )
  stopifnot(inherits(try(read_bundle(profile), silent = TRUE), "try-error"))
}

check_custom_layout()

# Exercise an entire build from an unrelated working directory. Only the large
# downloads and the native compiler are substituted; real app files, TOML,
# JSON and the published installer all cross the same filesystem boundaries.
check_external_build <- function() {
  root <- tempfile("external build ")
  stopifnot(dir.create(root))
  on.exit(unlink(root, recursive = TRUE))
  engine <- file.path(root, "engine")
  project <- file.path(root, "売上 app")
  output <- file.path(project, "dist")
  profile <- file.path(project, "shinytauri.toml")
  stopifnot(
    dir.create(file.path(engine, "src-tauri"), recursive = TRUE),
    dir.create(file.path(engine, "scripts")),
    dir.create(project),
    file.copy("src-tauri/tauri.conf.json", file.path(engine, "src-tauri")),
    file.copy("LICENSE", engine),
    file.copy("scripts/serve-bundled.R", file.path(engine, "scripts")),
    file.copy("examples/minimal/app.R", project),
    file.copy("bundles/minimal/rproj.lock", project)
  )
  lines <- c(
    'app = "."',
    'files = ["app.R"]',
    'lock = "rproj.lock"',
    'productName = "Sales"',
    'identifier = "jp.example.sales"',
    'version = "1.2.3"',
    'title = "売上画面"'
  )
  writeLines(lines, profile)
  subject <- new.env(parent = globalenv())
  sys.source("scripts/build-windows.R", subject)
  subject$stage_runtime <- function(...) NULL
  subject$stage_packages <- function(...) NULL
  subject$run_tauri <- function(engine, config, triple, target_dir) {
    contents <- jsonlite::read_json(config)
    win <- contents$app$windows[[1L]]
    stopifnot(
      contents$identifier == "jp.example.sales",
      contents$productName == "Sales",
      contents$version == "1.2.3",
      win$title == "売上画面",
      win$width == 800L,
      win$height == 600L
    )
    payload <- file.path(engine, "src-tauri", names(contents$bundle$resources))
    stopifnot(
      file.exists(file.path(payload, "app/app.R")),
      file.exists(file.path(payload, "serve-bundled.R")),
      !file.exists(file.path(payload, "app/rproj.lock"))
    )
    nsis <- file.path(target_dir, triple, "release/bundle/nsis")
    stopifnot(dir.create(nsis, recursive = TRUE))
    writeLines("new", file.path(nsis, "Sales_1.2.3_x64-setup.exe"))
  }
  old <- setwd(root)
  on.exit(setwd(old), add = TRUE, after = FALSE)
  subject$build_windows(profile, output, engine)
  published <- file.path(output, "Sales_1.2.3_x64-setup.exe")
  stopifnot(
    identical(readLines(published), "new"),
    identical(list.files(output), basename(published)),
    !dir.exists(file.path(project, ".rvenv"))
  )
  subject$run_tauri <- function(...) NULL
  stopifnot(
    inherits(
      try(subject$build_windows(profile, output, engine), silent = TRUE),
      "try-error"
    ),
    identical(readLines(published), "new")
  )

  # The output may be under the project, but cannot also be an input resource.
  writeLines(sub('"app.R"', '"app.R", "dist"', lines, fixed = TRUE), profile)
  stopifnot(grepl(
    "output",
    try(subject$build_windows(profile, output, engine), silent = TRUE),
    ignore.case = TRUE
  ))
}

check_external_build()

check_preflight <- function() {
  spec <- read_bundle("bundles/minimal/bundle.toml")
  root <- tempfile()
  stopifnot(dir.create(root))
  on.exit(unlink(root, recursive = TRUE))
  lock <- file.path(root, "rproj.lock")
  lines <- readLines(spec$lock)
  writeLines(sub("binary = true", "binary = false", lines, fixed = TRUE), lock)
  stopifnot(grepl("binary", try(windows_target(lock), silent = TRUE)))
  writeLines(
    sub('r_version = "4.6.2"', 'r_version = "3.0.0"', lines, fixed = TRUE),
    lock
  )
  stopifnot(grepl("3.0.0", try(windows_target(lock), silent = TRUE)))
  spec$app <- file.path(root, "app")
  spec$files <- c("app.R", "www")
  stopifnot(
    dir.create(file.path(spec$app, "www"), recursive = TRUE),
    file.create(file.path(spec$app, "app.R"))
  )
  if (.Platform$OS.type != "windows") {
    stopifnot(file.symlink(root, file.path(spec$app, "www/jump")))
    stopifnot(grepl(
      "link",
      try(stage_app(spec, file.path(root, "stage")), silent = TRUE)
    ))
  }
}
check_preflight()

check_cli_entry <- function() {
  root <- tempfile("engine path with spaces ")
  scripts <- file.path(root, "scripts")
  stopifnot(dir.create(scripts, recursive = TRUE))
  on.exit(unlink(root, recursive = TRUE))
  stopifnot(file.copy("scripts/build-windows.R", scripts))
  profile <- file.path(root, "bundle.toml")
  writeLines(
    c('app = "."', 'lock = "missing.lock"', 'files = ["app.R"]'),
    profile
  )
  messages <- suppressWarnings(system2(
    file.path(R.home("bin"), "Rscript"),
    c(
      "--vanilla",
      shQuote(file.path(scripts, "build-windows.R")),
      shQuote(profile)
    ),
    stdout = TRUE,
    stderr = TRUE
  ))
  stopifnot(
    attr(messages, "status") != 0L,
    any(grepl("rig proj lock", messages, fixed = TRUE))
  )
  stopifnot(file.create(file.path(root, "missing.lock")))
  cat('\nproductName = "NUL.txt"\n', file = profile, append = TRUE)
  stopifnot(grepl("productName", try(read_bundle(profile), silent = TRUE)))
}
check_cli_entry()
