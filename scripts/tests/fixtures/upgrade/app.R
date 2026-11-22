library(shiny)

saved_note <- file.path(
  Sys.getenv("LOCALAPPDATA"),
  "org.example.minimal",
  "note.txt"
)
ui <- fluidPage(
  h1("Updated counter 0.2.0"),
  textOutput("saved"),
  actionButton("count", "Count"),
  textOutput("total")
)
server <- function(input, output, session) {
  output$saved <- renderText(readLines(saved_note, encoding = "UTF-8"))
  output$total <- renderText(sprintf(
    "Updated count: %d",
    as.integer(input$count)
  ))
}
shinyApp(ui, server)
