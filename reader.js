var pages = [];
var comic = RegExp("/(\\w+)/reader.html").exec(window.location.pathname)[1];

let ws = new WebSocket(window.location.origin.replace("http", "ws") + "/msg");
let curPage = "";

function switchPage(page) {
    window.location.hash = page;
    document.getElementById("comic").setAttribute("src", `/${comic}/img/${encodeURIComponent(page)}`);
    curPage = page;
    current = pages.indexOf(page);
    if (current < pages.length - 1) {
        document.getElementById("comic-next").setAttribute("src", `/${comic}/img/${encodeURIComponent(pages[current + 1])}`);
    }
    window.scrollTo(0, 0);
    let select = document.getElementById("page-select");
    select.value = page;
}

function changePage(page) {
    ws.send(JSON.stringify({ "comic": comic, "page": page }));
    switchPage(page);
}

ws.onmessage = event => {
    let data = JSON.parse(event.data);
    if (data.comic === comic) {
        switchPage(data.page);
    }
};

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
    if (window.location.hash !== "") {
        changePage(window.location.hash.substr(1, window.location.hash.length - 1));
    } else {
        changePage(pages[0]);
    }
    document.getElementById("prev").onclick = _ => { prevPage(); };
    document.getElementById("comic-container").onclick = _ => { nextPage(); };
    document.getElementById("next").onclick = _ => { nextPage(); };

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
fetch(`/${comic}/img_list`).then(response => response.json()).then(body => pages = body).then(init);