extern crate dbus;
extern crate regex;
extern crate getopts;

use std::env;
use std::error::*;
use std::fmt;
use std::fs;
use std::io;

use dbus::{Connection, BusType, Message};
use dbus::arg::{Dict, Variant, RefArg};
use regex::Regex;
use getopts::Options;

const XFCONF_BUS  : &str = "org.xfce.Xfconf";
const XFCONF_PATH : &str = "/org/xfce/Xfconf";
const XFCONF_OBJ  : &str = "org.xfce.Xfconf";
const XFCONF_DESKTOP_CHANNEL : &str = "xfce4-desktop";
const XFCONF_BACKDROP_LIST_PATH : &str = "/backdrop/screen0/monitor0/image-path";

struct XFCEDesktop {
    conn: Connection,
    monitors: Vec<String>,
    _mon_re: Regex,
}

#[derive(Debug)]
enum XFConfError {
    CallError(String),
    DBusError(dbus::Error),
    RegexError(regex::Error),
    IOError(io::Error),
    BadType,
    NoData,
}

impl From<dbus::Error> for XFConfError {
    fn from(e: dbus::Error) -> Self {
        XFConfError::DBusError(e)
    }
}

impl From<String> for XFConfError {
    fn from(s: String) -> Self {
        XFConfError::CallError(s)
    }
}

impl From<regex::Error> for XFConfError {
    fn from(e: regex::Error) -> Self {
        XFConfError::RegexError(e)
    }
}

impl From<io::Error> for XFConfError {
    fn from(e: io::Error) -> Self {
        XFConfError::IOError(e)
    }
}

impl Error for XFConfError {
    fn description(&self) -> &str {
        match self {
            &XFConfError::CallError(ref s) => s.as_str(),
            &XFConfError::DBusError(ref dbe) => dbe.message().unwrap_or("<NoMessage>"),
            &XFConfError::RegexError(ref ree) => ree.description(),
            &XFConfError::BadType => "Incorrect type.",
            &XFConfError::NoData => "Message does not have data at that position",
            &XFConfError::IOError(ref e) => e.description(),
        }
    }

    fn cause(&self) -> Option<&Error> {
        match self {
            &XFConfError::RegexError(ref ree) => ree.cause(),
            _ => None
        }
    }
}

impl fmt::Display for XFConfError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "XFConf Error : {}", self.description())
    }
}

impl XFCEDesktop {
    fn new() -> Result<XFCEDesktop, XFConfError> {
        let mut xfcedesktop = XFCEDesktop {
            conn: Connection::get_private(BusType::Session)?,
            monitors: Vec::new(),
            _mon_re: Regex::new(r"/backdrop/screen0/monitor(.*)/workspace0/color-style")?,
        };

        xfcedesktop.monitors = xfcedesktop.get_monitors()?;
        Ok(xfcedesktop)
    }

    fn mk_call(&self, method: &str) -> Result<Message, XFConfError> {
        Ok(Message::new_method_call(XFCONF_BUS, XFCONF_PATH, XFCONF_OBJ, method)?)
    }

    #[allow(dead_code)]
    fn call_method(&self, msg: Message) -> Result<Message, XFConfError> {
        Ok(self.conn.send_with_reply_and_block(msg, 2000)?)
    }

    fn get_background(&self, _monitor: &str, _workspace: &str) -> Result<String, XFConfError> {
        let prop_path = format!("/backdrop/screen0/monitor{monitor}/workspace{workspace}/last-image",
                                monitor = _monitor, workspace = _workspace);
        let m = self.call_method(self.mk_call("GetProperty")?.append2(XFCONF_DESKTOP_CHANNEL, &prop_path))?;
        let v: Variant<Box<RefArg>> = m.get1().ok_or(XFConfError::NoData)?;
        let z: &str = v.as_str().ok_or(XFConfError::BadType)?;
        Ok(z.to_string())
    }

    #[allow(dead_code)]
    fn set_background(&self, _monitor: &str, _workspace: &str, _image_path: &str) -> Result<(), XFConfError> {
        let prop_path = format!("/backdrop/screen0/monitor{monitor}/workspace{workspace}/last-image",
                                monitor = _monitor, workspace = _workspace);
        let img_path_v = Variant(_image_path);
        self.call_method(self.mk_call("SetProperty")?.append3(XFCONF_DESKTOP_CHANNEL, &prop_path, &img_path_v))?;
        Ok(())
    }

    /// Scrape monitors from the property list.
    fn get_monitors(&self) -> Result<Vec<String>,XFConfError> {
        let m = self.call_method(self.mk_call("GetAllProperties")?.append2(XFCONF_DESKTOP_CHANNEL, "/backdrop/screen0"))?;
        let z: Dict<&str, Variant<Box<RefArg>>, _> = m.get1().unwrap();
        let mut mons_set: Vec<String> = z.map(|(x,_)| x)
            .map(|fld| self._mon_re.captures(fld))
            .filter(Option::is_some)
            .map(|c| c.unwrap().get(1).unwrap().as_str().to_string())
            .collect();
        mons_set.sort();
        mons_set.dedup();
        Ok(mons_set)
    }

    fn get_list(&self) -> Result<String, XFConfError> {
        let m = self.call_method(self.mk_call("GetProperty")?.append2(XFCONF_DESKTOP_CHANNEL, XFCONF_BACKDROP_LIST_PATH))?;
        let v: Variant<Box<RefArg>> = m.get1().ok_or(XFConfError::NoData)?;
        let z: &str = v.as_str().ok_or(XFConfError::BadType)?;
        Ok(z.to_string())
    }

    fn is_list_valid(&self, _list: &str) -> Result<(), XFConfError> {
        let _list_attr = fs::metadata(_list)?;
        Ok(())
    }

    fn set_list(&self, _list: &str) -> Result<(), XFConfError> {
        self.is_list_valid(_list)?;

        let list_v = Variant(_list);
        self.call_method(self.mk_call("SetProperty")?.append3(XFCONF_DESKTOP_CHANNEL, XFCONF_BACKDROP_LIST_PATH, list_v))?;
        Ok(())
    }
}

fn print_usage(progname: &str, opts: Options) {
    let brief = format!("Usage: {} [options] [IMGFILE]:[IMGFILE]:[IMGFILE].. ", progname);
    print!("{}", opts.usage(&brief));
}

fn do_query(xfconf: &XFCEDesktop) {
    println!("Monitors:");
    for m in xfconf.monitors.iter() {
        println!("{} Img = {}", m, xfconf.get_background(m, "0").unwrap());
    }
}

fn do_setlist(xfconf: &XFCEDesktop, listfile: &String) {
    match xfconf.get_list() {
        Ok(list) => println!("Current list is : {}", list),
        Err(e) =>   println!("Could not get list, {}", e),
    }
    println!("do_setlist(): Setting list = {}", listfile);
    match xfconf.set_list(listfile) {
        Err(e) => println!("Error setting list path to ({}): {}", listfile, e),
        Ok(_) => ()
    }
}

fn do_setimg(xfconf: &XFCEDesktop, imgfile_args: &Vec<String>) {
    let imgfiles_collated = imgfile_args.join(":");
    let imgfiles: Vec<&str> = imgfiles_collated.split(":").collect();
    let imgpairs = imgfiles.iter()
        .zip(xfconf.monitors.iter())
        .filter(|&(i,_)| !i.is_empty());

    for (i, m) in imgpairs {
        println!("do_setimg(): Monitor: {}, Image: {}", m, i);
        match xfconf.set_background(m, "0", i) {
            Err(e) => println!("Failed : {} ", e),
            Ok(_) => ()
        }
    }
}

///
/// Current usage: prog <image-file>
///
fn main() {
    let args: Vec<String> = env::args().collect();
    let xfconf = XFCEDesktop::new().unwrap();
    let progname = args[0].clone();

    let mut opts = Options::new();

    opts.optopt  ("l", "listfile", "set backdrop list file name", "LISTFILE");
    opts.optflag ("q", "query",    "query the current setting");
    opts.optflag ("h", "help",     "this help");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => {
            println!("{}", f.to_string());
            print_usage(&progname, opts);
            return;
        }
    };

    if matches.opt_present("h") {
        print_usage(&progname, opts);
        return;
    }

    if matches.opt_present("q") {
        do_query(&xfconf);
        return;
    }

    if matches.opt_present("l") {
        if let Some(listfile) = matches.opt_str("l") {
            do_setlist(&xfconf, &listfile);
        }
        else {
            print_usage(&progname, opts);
        }
    }

    if !matches.free.is_empty() {
        do_setimg(&xfconf, &matches.free);
    };
}
