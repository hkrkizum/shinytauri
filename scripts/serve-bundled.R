# Start only after the desktop host owns this process and its future workers.
input <- file("stdin", open = "r")
stopifnot(identical(readLines(input, n = 1L), "start"))
close(input)

# The UI and its reactive output use Japanese regardless of the Windows locale.
if (!l10n_info()[["UTF-8"]]) {
  stopifnot(nzchar(Sys.setlocale("LC_CTYPE", ".UTF-8")))
}

ready <- Sys.getenv("SHINYTAURI_READY_FILE")
stopifnot(nzchar(ready))
shiny::runApp(
  ".",
  host = "127.0.0.1",
  port = NULL,
  launch.browser = function(url) {
    pending <- paste0(ready, ".tmp")
    writeLines(url, pending)
    stopifnot(file.rename(pending, ready))
  }
)
