let pages = [];
let curPage = "";
let mirror = null;
let failed_mirror_hits = 0;
let comic = RegExp("/(\\w+)/reader.html").exec(window.location.pathname)[1];

let ws = null;

function switchPage(page) {
    let root = `/img/${comic}`;
    if (mirror && failed_mirror_hits < 10) {
        root = `${mirror}/${comic}`;
    }
    window.location.hash = page;
    document.getElementById("comic").setAttribute("src", `${root}/${encodeURIComponent(page)}`);
    curPage = page;
    current = pages.indexOf(page);
    if (current < pages.length - 1) {
        document.getElementById("comic-next").setAttribute("src", `${root}/${encodeURIComponent(pages[current + 1])}`);
    }
    window.scrollTo(0, 0);
    let select = document.getElementById("page-select");
    select.value = page;
}

function changePage(page) {
    ws.send(JSON.stringify({ "comic": comic, "page": page }));
    switchPage(page);
}

function nextPage() {
    let current = pages.indexOf(curPage);
    if (current < pages.length - 1) {
        changePage(pages[current + 1]);
    }
}
function prevPage() {
    let current = pages.indexOf(curPage);
    if (current > 0) {
        changePage(pages[current - 1]);
    }
}

function init() {
    ws = new WebSocket(window.location.origin.replace("http", "ws") + "/msg");

    ws.onopen = (event) => {
        if (window.location.hash !== "") {
            changePage(decodeURIComponent(window.location.hash.substr(1, window.location.hash.length - 1)));
        } else {
            changePage(pages[0]);
        }
        document.getElementById("prev").onclick = _ => { prevPage(); };
        document.getElementById("comic-container").onclick = _ => { nextPage(); };
        document.getElementById("next").onclick = _ => { nextPage(); };

        let skip_mirror = (error) => {
            let src = error.originalTarget.getAttribute("src");
            if (!src.startsWith("/")) {
                failed_mirror_hits += 1;
                error.originalTarget.setAttribute("src", src.replace(mirror, "/img"));
            }
        }
        document.getElementById("comic").onerror = skip_mirror;
        document.getElementById("comic-next").onerror = skip_mirror;

        document.addEventListener("keydown", event => {
            if (event.code == "ArrowLeft") {
                prevPage();
                event.preventDefault();
            } else if (event.code == "ArrowRight") {
                nextPage();
                event.preventDefault();
            }
        });

        let select = document.getElementById("page-select");
        for (let page of pages) {
            let opt = document.createElement("option");
            opt.text = page;
            if (page == curPage) {
                opt.selected = true;
            }
            select.appendChild(opt);
        }
        select.onchange = (ev) => {
            changePage(ev.target.value);
        }
    }

    ws.onmessage = event => {
        let data = JSON.parse(event.data);
        if (data.comic === comic) {
            switchPage(data.page);
        }
    };
}
fetch(`/${comic}/img_list`)
    .then(response => response.json())
    .then(body => { pages = body.pages; mirror = body.mirror })
    .then(init);