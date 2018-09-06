///
/// Utility to emulate desktop backdrop 'list' behaviour of
/// pre 4.12 XFCE.
///
/// TODO:
/// * Multiple Workspaces
/// * Daemon
///
extern crate dbus;
extern crate regex;
extern crate getopts;
extern crate rand;

use std::env;
use std::error::*;
use std::fmt;
use std::fs::{metadata, File};
use std::io;
use std::io::Read;

use dbus::{Connection, BusType, Message};
use dbus::arg::{Dict, Variant, RefArg};
use regex::Regex;
use getopts::Options;
use rand::{Rng, FromEntropy};
use rand::rngs::SmallRng;

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

/// Wrapped error used in this program.
#[derive(Debug)]
enum XFConfError {
    /// String error from dbus::Message functions
    CallError(String),
    /// Wrapped Error from dbus::Connection functions and many others.
    DBusError(dbus::Error),
    /// Wrapped regex Error
    RegexError(regex::Error),
    /// Wrapped error from file operations
    IOError(io::Error),
    /// Could not pull out the data of expected type from a response variant.
    BadType,
    /// Indicating that from Message::get() gave a 'None'.
    NoData,
    /// Could not pick one images
    NoImage,
}

// Transform impl's for various error types used to XFConfError.
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
            &XFConfError::NoImage => "Could not pick an image from the list",
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

    /// Gets the list file name saved in an 'old' config variable
    fn get_list(&self) -> Result<String, XFConfError> {
        let m = self.call_method(self.mk_call("GetProperty")?.append2(XFCONF_DESKTOP_CHANNEL, XFCONF_BACKDROP_LIST_PATH))?;
        let v: Variant<Box<RefArg>> = m.get1().ok_or(XFConfError::NoData)?;
        let z: &str = v.as_str().ok_or(XFConfError::BadType)?;
        Ok(z.to_string())
    }

    /// Filter out comments and get list of filenames.
    fn get_image_names(&self, _list: &str) -> Result<Vec<String>, XFConfError> {
        let mut contents = String::new();
        File::open(_list)?.read_to_string(&mut contents)?;
        Ok(contents.lines().map(str::trim).filter(|x| !x.starts_with("#")).map(str::to_string).collect::<Vec<String>>())
    }

    /// Pick one image from a list of names provided. Checking if they are actual
    /// file system entries. TODO: 'is file' ?
    fn pick_image(&self, image_names: &Vec<String>) -> Result<String, XFConfError> {
        let mut _rng = SmallRng::from_entropy();
        let mut imgs = image_names.iter().collect::<Vec<&String>>();
        imgs.sort();
        imgs.dedup();
        let mut lsize = imgs.len();

        while lsize > 0 {
            let picked_idx = _rng.gen_range(0, lsize);
            match metadata(&imgs[picked_idx]) {
                Err(_) => {
                    imgs.remove(picked_idx);
                    lsize = imgs.len();
                },
                Ok(_) => {
                    return Ok(imgs[picked_idx].to_string());
                }
            }
        }

        Err(XFConfError::NoImage)
    }

    #[allow(dead_code)]
    fn rotate_background_for_monitor(&self, _monitor: &str, _workspace: &str, image_list: &Vec<String>) -> Result<(), XFConfError> {
        self.set_background(_monitor, _workspace, self.pick_image(image_list)?.as_str())?;
        Ok(())
    }

    /// Randomly usable files from given list and set the backdrop for all the monitors
    /// found in xfconf-desktop configuration.
    fn rotate_background(&self, _workspace: &str, image_list: &Vec<String>) -> Result<(), XFConfError> {
        for m in &self.monitors {
            let img = self.pick_image(image_list)?;
            println!("Setting image for monitor{} : {}", m, &img);
            self.set_background(m.as_str(), _workspace, &img)?;
        }
        Ok(())
    }

    /// Used the saved list file to set backdrops for all monitors.
    fn rotate_from_saved(&self) -> Result<(), XFConfError> {
        let image_names = self.get_image_names(self.get_list()?.as_str())?;
        self.rotate_background("0", &image_names)?;
        Ok(())
    }

    /// Sets list file name to xfce desktop config registry
    /// Additionally checks if the list file is readable and atleast one image file exists in it.
    fn set_list(&self, _list: &str) -> Result<(), XFConfError> {
        let _list_attr = metadata(_list)?;

        let image_list =self.get_image_names(_list)?;
        self.pick_image(&image_list)?;

        let list_v = Variant(_list);
        self.call_method(self.mk_call("SetProperty")?.append3(XFCONF_DESKTOP_CHANNEL, XFCONF_BACKDROP_LIST_PATH, list_v))?;
        Ok(())
    }
}

fn print_usage(progname: &str, opts: Options) {
    let brief = format!("Usage: {} [options] [IMGFILE]:[IMGFILE]:[IMGFILE].. ", progname);
    print!("{}", opts.usage(&brief));
}

/// Print out the currently set list file name and file names of backdrop images
fn do_query(xfconf: &XFCEDesktop) {
    match xfconf.get_list() {
        Ok(list) => println!("Current list file is : {}", list),
        Err(e) =>   println!("Could not get list, {}", e),
    }
    println!("Current image file(s) set:");
    for m in xfconf.monitors.iter() {
        println!("\t{} : {}", m, xfconf.get_background(m, "0").unwrap());
    }
}

/// Shows current list, sets provided list if atleast one line in the list
/// is a file. The `rotate` argument tells the function if it should
/// attempt to set backdrops from the newly set list.
fn do_setlist(xfconf: &XFCEDesktop, listfile: &String, rotate: bool) {
    match xfconf.get_list() {
        Ok(list) => println!("Current list is : {}", list),
        Err(e) =>   println!("Could not get list, {}", e),
    }
    println!("do_setlist(): Setting list = {}", listfile);
    if let Err(e) = xfconf.set_list(listfile) {
        println!("Error setting list path to ({}): {}", listfile, e);
    }
    else if rotate {
        do_rotate(&xfconf);
    }
}

/// Accumulates bunch of arguments which are ':' separated and maps
/// them on to the monitors inferred from the current xfce config.
/// The monitor list is sorted which allows use of colon ':' to
/// set an image file as backdrop for a specific monitor.
///
/// If the '-q' option shows monitors in an order like 'DP1', 'HDMI1'
/// then the argument ':xyz.jpg' would set the backdrop for 'HDMI1'
/// xyz.jpg
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

fn do_rotate(xfconf: &XFCEDesktop) {
    if let Err(e) = xfconf.rotate_from_saved() {
        println!("Failed: {}", e);
    }
}

///
/// Current usage: prog -l <list-file> -q -h -n image-file:image-file:...
///
fn main() {
    let args: Vec<String> = env::args().collect();
    let xfconf = XFCEDesktop::new().unwrap();
    let progname = args[0].clone();

    let mut opts = Options::new();

    opts.optopt  ("l", "listfile", "Set backdrop list file name", "LISTFILE");
    opts.optflag ("q", "query",    "Query the current setting");
    opts.optflag ("h", "help",     "This help");
    opts.optflag ("n", "norotate", "Do not set the backgrounds from list file while setting it");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => {
            println!("{}", f.to_string());
            print_usage(&progname, opts);
            return;
        }
    };

    // Help
    if matches.opt_present("h") {
        print_usage(&progname, opts);
        return;
    }

    // Query current
    if matches.opt_present("q") {
        do_query(&xfconf);
        return;
    }

    // Set list
    if matches.opt_present("l") {
        if let Some(listfile) = matches.opt_str("l") {
            do_setlist(&xfconf, &listfile, (!matches.opt_present("n")) && matches.free.is_empty());
            // Some more image arguments
            if matches.free.is_empty() {
                return;
            }
        }
        // No list file given
        else {
            print_usage(&progname, opts);
        }
    }

    // Remaining args as a collection of image:image:... image:image..
    if !matches.free.is_empty() {
        do_setimg(&xfconf, &matches.free);
    }
    // No args means rotate
    else if !matches.opt_present("n") {
        do_rotate(&xfconf);
    }
}
