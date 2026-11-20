app <- rhino::app()
app$onStart <- function() {
  mirai::daemons(4L)
  shiny::onStop(function() mirai::daemons(0L), session = NULL)
}
app
