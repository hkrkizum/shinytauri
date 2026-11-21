library(shiny)

shinyApp(
  ui = fluidPage(
    tags$link(rel = "stylesheet", href = "style.css"),
    actionButton("count", "1 を足す"),
    textOutput("total")
  ),
  server = function(input, output, session) {
    output$total <- renderText(as.integer(input$count))
  }
)
