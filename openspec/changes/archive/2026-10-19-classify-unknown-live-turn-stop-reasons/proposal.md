# Treat unknown live turn stop reasons as failures

The live Pager terminal path currently treats every stop reason outside three special cases as a successful turn. An interrupted or newly introduced reason can therefore render a completed marker and success notification. Make live terminal classification explicit so only known successful ACP terminal reasons complete successfully; unknown reasons remain visible as failures.
