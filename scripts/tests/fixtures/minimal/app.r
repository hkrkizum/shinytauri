library(shiny)
source("counter-helper.r", local = TRUE)
gate <- Sys.getenv("SHINYTAURI_SMOKE_GATE")
if (nzchar(gate)) {
  deadline <- Sys.time() + 90
  while (!file.exists(gate)) {
    if (Sys.time() >= deadline) {
      stop("Acceptance gate timed out")
    }
    Sys.sleep(0.1)
  }
  action <- readLines(gate, warn = FALSE)
  if (identical(action, "timeout")) {
    # Keep a real R worker alive when the desktop host reaches its deadline.
    expression <- paste(
      "ready <- paste0(Sys.getenv('SHINYTAURI_SMOKE_GATE'), '.worker-ready');",
      "writeLines(as.character(Sys.getpid()), paste0(ready, '.tmp'));",
      "stopifnot(file.rename(paste0(ready, '.tmp'), ready));",
      "Sys.sleep(120)"
    )
    system2(
      file.path(R.home("bin"), "Rscript.exe"),
      c("--vanilla", "-e", shQuote(expression)),
      wait = FALSE
    )
    Sys.sleep(120)
  }
  if (identical(action, "fail")) {
    stop("Acceptance fixture startup failure")
  }
}

ui <- fluidPage(
  tags$link(rel = "stylesheet", href = "style.css"),
  actionButton("count", "Count"),
  textOutput("total"),
  textOutput("runtime")
)
server <- function(input, output, session) {
  output$runtime <- renderText(paste("R", getRversion()))
  output$total <- renderText(format_counter(input$count))
}
shinyApp(ui, server)
