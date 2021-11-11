# Shared comic reader

Creates a web server that serves image sets to one or more clients. When one of those clients turns a page, all other clients who are viewing that image set also change pages.

## To run:

Run the binary from the folder where the html and js files are located. Images should be in folders with url-safe names in the directory specified by the `--folder` command-line option (default current directory). Images will be served in the order that Windows Explorer sorts them in - lexicographic order, but with consecutive sequences of digits treated as numbers to be compared.

The optional `--mirror` command-line option allows you to have clients download the images from another site instead of from your server. The folder and file structure of the mirror should be the same as the files in your file system. For example, if your images are stored in `D:\Users\me\comics\<comic_name>\<image_file>` then you can launch the server with `--folder D:\Users\me\comics --mirror https://my-mirror-s3-bucket.s3.us-east-2.amazonaws.com` to have it serve the images from `https://my-mirror-s3-bucket.s3.us-east-2.amazonaws.com/<comic_name>/<image_file>`. Any files not present in the mirror will be served directly, and if there are too many misses then the client will stop trying to use the mirror.

All command-line options may also be loaded from `config.json`.

## To use:

Connect to the server through the port specified by the `--port` command-line option (default 30000). Left/right arrow keys and clicking on the left/right bars moves back/forwards, clicking on the image moves forwards, and choosing from the dropdown in the header jumps to that page. When one client changes pages, all other clients who are viewing the same comic also change pages.

## License:

IDGAF, but since that doesn't hold legal weight, MIT + Apache 2.0 dual license like most Rust projects.
