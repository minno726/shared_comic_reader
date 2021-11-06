#Shared comic reader

Creates a web server that serves image sets to one or more clients. When one of those clients turns a page, all other clients who are viewing that image set also change pages.

##To run:

Run the binary from the folder where the html and js files are located. Images should be in folders with url-safe names in the directory specified by the `--folder` command-line option (default current directory). Images will be served in the order that Windows Explorer sorts them in - lexicographic order, but with consecutive sequences of digits treated as numbers to be compared.

##To use:

Connect to the server through the port specified by the `--port` command-line option (default 30000). Left/right arrow keys and clicking on the left/right bars moves back/forwards, clicking on the image moves forwards, and choosing from the dropdown in the header jumps to that page. When one client changes pages, all other clients who are viewing the same comic also change pages.

##License:

IDGAF, but since that doesn't hold legal weight, MIT + Apache 2.0 dual license like most Rust projects.