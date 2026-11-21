source("scripts/build-windows.R")

expect_bundle_error <- function(expr, text) {
  error <- tryCatch(
    {
      force(expr)
      NULL
    },
    error = identity
  )
  stopifnot(
    inherits(error, "error"),
    grepl(text, conditionMessage(error), fixed = TRUE)
  )
}

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

# Tauri enumerates files, so an empty directory must survive that boundary.
check_empty_directories <- function() {
  root <- tempfile("empty directories ")
  app <- file.path(root, "source")
  static <- file.path(app, "assets", "static")
  stopifnot(
    dir.create(static, recursive = TRUE),
    dir.create(file.path(app, "legacy", "nested"), recursive = TRUE),
    file.create(file.path(app, "app.R")),
    file.copy("bundles/minimal/rproj.lock", root)
  )
  on.exit(unlink(root, recursive = TRUE))
  profile <- file.path(root, "bundle.toml")
  lines <- c(
    'app = "source"',
    'lock = "rproj.lock"',
    'files = ["app.R", "legacy"]'
  )
  writeLines(c(lines, 'emptyDirs = ["assets/static"]'), profile)
  writeLines("source only", file.path(static, ".secret"))
  old <- setwd(tempdir())
  on.exit(setwd(old), add = TRUE, after = FALSE)
  spec <- read_bundle(profile)
  check_stage <- function(name) {
    dest <- file.path(root, name)
    stage_app(spec, dest)
    for (dir in c("assets/static", "legacy/nested")) {
      staged <- file.path(dest, dir)
      stopifnot(
        identical(
          list.files(staged, all.files = TRUE, no.. = TRUE),
          ".gitkeep"
        ),
        file.size(file.path(staged, ".gitkeep")) == 0
      )
    }
  }
  check_stage("first")
  writeLines("later addition", file.path(static, "new.txt"))
  check_stage("second")
  stopifnot(!file.exists(file.path(static, ".gitkeep")))
  writeLines("explicit contents", file.path(static, ".gitkeep"))
  spec$files <- c("app.R", "assets/static/.gitkeep")
  dest <- file.path(root, "chosen")
  stage_app(spec, dest)
  stopifnot(identical(
    readLines(file.path(dest, "assets/static/.gitkeep")),
    "explicit contents"
  ))
  # Selecting a parent directory in files still opts in to its complete contents.
  spec$files <- c("app.R", "assets")
  dest <- file.path(root, "recursive")
  stage_app(spec, dest)
  stopifnot(identical(
    readLines(file.path(dest, "assets/static/new.txt")),
    "later addition"
  ))
  spec$files <- "app.R"
  expect_bundle_error(
    stage_app(spec, file.path(root, "collision"), file.path(static, "dist")),
    "output directory"
  )
  for (bad in c("missing", "app.R")) {
    spec$emptyDirs <- bad
    expect_bundle_error(
      stage_app(spec, file.path(root, paste0("bad-", bad))),
      "directory"
    )
  }
  writeLines(c(lines, 'emptyDirs = ["../outside"]'), profile)
  expect_bundle_error(read_bundle(profile), "relative")
  if (.Platform$OS.type != "windows") {
    stopifnot(file.symlink(static, file.path(app, "shortcut")))
    spec$emptyDirs <- "shortcut"
    expect_bundle_error(stage_app(spec, file.path(root, "symlink")), "link")
  }
}

check_empty_directories()

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
  # The project itself is a valid output directory. Protect a selected source
  # file if its name matches the actual installer produced by Tauri.
  subject$build_windows(profile, project, engine)
  in_project <- file.path(project, basename(published))
  stopifnot(identical(readLines(in_project), "new"))
  writeLines("source", in_project)
  selected <- sub(
    '"app.R"',
    paste0('"app.R", "', basename(published), '"'),
    lines,
    fixed = TRUE
  )
  writeLines(selected, profile)
  expect_bundle_error(
    subject$build_windows(profile, project, engine),
    "overwrite"
  )
  stopifnot(identical(readLines(in_project), "source"))
  writeLines(lines, profile)
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
  expect_bundle_error(
    subject$build_windows(profile, output, engine),
    "output directory"
  )
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
  expect_bundle_error(windows_target(lock), "binary")
  writeLines(
    sub('r_version = "4.6.2"', 'r_version = "3.0.0-beta"', lines, fixed = TRUE),
    lock
  )
  expect_bundle_error(windows_target(lock), "major.minor.patch")
  spec$app <- file.path(root, "app")
  spec$files <- c("app.R", "www")
  stopifnot(
    dir.create(file.path(spec$app, "www"), recursive = TRUE),
    file.create(file.path(spec$app, "app.R"))
  )
  # Match shiny::runApp's case-insensitive directory entry points, including
  # an HTML UI served by www/ and server.R without a separate ui.R.
  spec$files <- "www"
  expect_bundle_error(
    stage_app(spec, file.path(root, "no-entry")),
    "entry point"
  )
  stopifnot(file.create(file.path(spec$app, "SERVER.r")))
  spec$files <- c("SERVER.r", "www")
  stage_app(spec, file.path(root, "html-ui"))
  if (.Platform$OS.type != "windows") {
    stopifnot(file.symlink(root, file.path(spec$app, "www/jump")))
    expect_bundle_error(stage_app(spec, file.path(root, "stage")), "link")
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
  expect_bundle_error(read_bundle(profile), "productName")
}
check_cli_entry()
source("scripts/tests/test-runtime-deps.R")
source("scripts/tests/test-portable-runtime.R")
