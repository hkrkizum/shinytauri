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
