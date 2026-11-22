library(shiny)
gate <- Sys.getenv("SHINYTAURI_SMOKE_GATE")
if (nzchar(gate)) {
  deadline <- Sys.time() + 90
  while (!file.exists(gate)) {
    if (Sys.time() >= deadline) {
      stop("Acceptance gate timed out")
    }
    Sys.sleep(0.1)
  }
  if (identical(readLines(gate, warn = FALSE), "fail")) {
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
  output$total <- renderText(sprintf(
    "%s (stringr %s)",
    stringr::str_pad(as.integer(input$count), 4, pad = "0"),
    utils::packageVersion("stringr")
  ))
}
shinyApp(ui, server)
