///
/// Utility to emulate desktop backdrop 'list' behaviour of
/// pre 4.12 XFCE.
///
/// TODO:
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
use dbus::arg::{Dict, Variant, RefArg, cast};
use regex::Regex;
use getopts::Options;
use rand::{Rng, FromEntropy};
use rand::rngs::SmallRng;

const XFCONF_BUS  : &str = "org.xfce.Xfconf";
const XFCONF_PATH : &str = "/org/xfce/Xfconf";
const XFCONF_OBJ  : &str = "org.xfce.Xfconf";
const XFCONF_DESKTOP_CHANNEL : &str = "xfce4-desktop";
const XFCONF_BACKDROP_LIST_PATH : &str = "/backdrop/screen0/monitor0/image-path";
const XFCONF_SINGLE_WORKSPACE_MODE : &str = "/backdrop/single-workspace-mode";
const XFCONF_SINGLE_WORKSPACE_NUMBER : &str = "/backdrop/single-workspace-number";

struct XFCEDesktop {
    conn: Connection,
    monitors: Vec<String>,
    workspace_count: u64,
    single_mode: bool,
    single_workspace: u64,
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
    /// COuld not figure out the monitors and workspace info
    NoDesktopInfo,
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
            &XFConfError::NoDesktopInfo => "Could get XFCE4 desktop and workspace configuration",
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
            workspace_count: 0,
            single_mode: true,
            single_workspace: 0,
        };

        xfcedesktop.refresh_monitors_and_workspaces()?;
        xfcedesktop.refresh_single_workspace_info()?;
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
    fn refresh_monitors_and_workspaces(&mut self) -> Result<(),XFConfError> {
        let _mon_re = Regex::new(r"/backdrop/screen0/monitor(.*)/workspace(.*)/color-style")?;

        let m = self.call_method(self.mk_call("GetAllProperties")?.append2(XFCONF_DESKTOP_CHANNEL, "/backdrop/screen0"))?;
        let props: Dict<&str, Variant<Box<RefArg>>, _> = m.get1().ok_or(XFConfError::NoData)?;
        let prop_keys: Vec<String> = props.map(|(x,_)| x.to_string()).collect();

        let mut mons_set: Vec<String> = prop_keys.iter().map(|fld| _mon_re.captures(fld))
            .filter(Option::is_some)
            .map(Option::unwrap)
            .filter(|c| c.len() > 1)
            .map(|c| c.get(1).unwrap().as_str().to_string())
            .collect();

        mons_set.sort();
        mons_set.dedup();

        if mons_set.len() <= 0 {
            return Err(XFConfError::NoDesktopInfo);
        }

        let workspaces_re =  Regex::new(&format!("/backdrop/screen0/monitor{monitor}/workspace.*/last-image",
                                                 monitor = &mons_set[0].as_str()))?;
        let workspace_count = prop_keys.iter().filter(|fld| workspaces_re.is_match(fld)).count();

        self.monitors = mons_set;
        self.workspace_count = workspace_count as u64;

        Ok(())
    }

    fn refresh_single_workspace_info(&mut self) -> Result<(),XFConfError> {
        let m_sws_mode = self.call_method(self.mk_call("GetProperty")?.append2(XFCONF_DESKTOP_CHANNEL, XFCONF_SINGLE_WORKSPACE_MODE));
        let m_sws_num = self.call_method(self.mk_call("GetProperty")?.append2(XFCONF_DESKTOP_CHANNEL, XFCONF_SINGLE_WORKSPACE_NUMBER));

        // Those 'single-workspace-*' props might not be present always.
        if m_sws_mode.is_err() || m_sws_num.is_err() {
            self.single_mode = true;
            self.single_workspace = 0;
        }
        else {
            let v_mode: Variant<Box<RefArg>> = m_sws_mode?.get1().ok_or(XFConfError::NoData)?;
            let v_num: Variant<Box<RefArg>> = m_sws_num?.get1().ok_or(XFConfError::NoData)?;
            let mode = cast::<bool>(&v_mode.0).ok_or(XFConfError::NoData)?;
            let num = v_num.as_i64().ok_or(XFConfError::NoData)?;

            self.single_mode = *mode;
            self.single_workspace = if num < 0 { 0 } else { num as u64 };
        }
        Ok(())
    }

    fn set_single_workspace_info(&self, mode: bool, workspace_number: Option<i64>) -> Result<(),XFConfError> {
        self.call_method(self.mk_call("SetProperty")?
                         .append3(XFCONF_DESKTOP_CHANNEL,
                                  XFCONF_SINGLE_WORKSPACE_MODE,
                                  Variant(mode)))?;
        if let Some(n) = workspace_number {
            self.call_method(self.mk_call("SetProperty")?
                             .append3(XFCONF_DESKTOP_CHANNEL,
                                      XFCONF_SINGLE_WORKSPACE_NUMBER,
                                      Variant(n)))?;
        }
        Ok(())
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
            println!("monitor{}, workspace-{} : {}", m, _workspace, &img);
            self.set_background(m.as_str(), _workspace, &img)?;
        }
        Ok(())
    }

    /// Used the saved list file to set backdrops for all monitors.
    fn rotate_from_saved(&self) -> Result<(), XFConfError> {
        let image_names = self.get_image_names(self.get_list()?.as_str())?;
        if self.single_mode {
            let wsp = format!("{}", self.single_workspace);
            self.rotate_background(&wsp, &image_names)?;
        } else {
            for wsp in self.workspace_names().iter() {
                self.rotate_background(&wsp, &image_names)?;
            }
        }
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

    fn workspace_names(&self) -> Vec<String> {
        (0..self.workspace_count).map(|x| format!("{}", x)).collect()
    }
}

fn print_usage(progname: &str, opts: Options) {
    let brief = format!("Usage: {} [options] [IMGFILE]:[IMGFILE]:.. [IMGFILE]:..", progname)
        + &format!("\n\nIMGFILES are mapped into (monitor, workspace) pairs.")
        + &format!("\nThe monitors are sorted as indicated by the '-q' option");
    print!("{}", opts.usage(&brief));
}

fn do_fetch_list(xfconf: &XFCEDesktop) {
    match xfconf.get_list() {
        Ok(list) => println!("Current list file is : {}", list),
        Err(e) =>   println!("Could not get list, {}", e),
    }
}

/// Print out the currently set list file name and file names of backdrop images
fn do_query(xfconf: &XFCEDesktop) {
    do_fetch_list(xfconf);
    println!("Current image file(s) set:");
    for m in xfconf.monitors.iter() {
        println!(" {} : Mode = {}", m, if xfconf.single_mode { "single" } else { "seperate" } );
        for wsp_idx in 0..xfconf.workspace_count {
            let wsp = format!("{}", wsp_idx);
            println!("\tworkspace {}{}: {}", wsp_idx,
                     if wsp_idx == xfconf.single_workspace { "*" } else { " " },
                     xfconf.get_background(m, &wsp).unwrap());
        }
    }

    println!("Single backdrop mode = {}", xfconf.single_mode);
    if xfconf.single_mode {
        println!("Single backdrop mode workspace = {}", xfconf.single_workspace);
    }
}

/// Shows current list, sets provided list if atleast one line in the list
/// is a file. The `rotate` argument tells the function if it should
/// attempt to set backdrops from the newly set list.
fn do_setlist(xfconf: &XFCEDesktop, listfile: &String, rotate: bool) {
    do_fetch_list(xfconf);
    println!("do_setlist(): Setting list = {}", listfile);
    if let Err(e) = xfconf.set_list(listfile) {
        println!("Error setting list path to ({}): {}", listfile, e);
    }
    else if rotate {
        do_rotate(&xfconf);
    }
}

/// Accepts ':' separated list of image file names and maps them on to
/// the (monitor, workspace) inferred from the current xfce config.
/// The (monitor, workspace) list is sorted which allows use of colon
/// ':' to set an image file as backdrop for a specific monitor and
/// workspace.
///
/// The repeat option repeats the image list on to the list of
/// (monitor, workspace) pairs. If absent and the image list does not
/// span the entire monitor x workspace range the remaining slots are
/// untouched.
///
/// If the '-q' option shows monitors in an order like 'DP1', 'HDMI1'
/// and totally 2 workspaces are there then the argument ':::xyz.jpg'
/// would set the backdrop for 'HDMI1' and workspace 1 (workspace
/// indices starts at 0) to xyz.jpg.
fn do_setimg(xfconf: &XFCEDesktop, imgfiles_collated: &String, repeat: bool) {
    let workspaces = xfconf.workspace_names();
    let all_workspaces = xfconf.monitors.iter()
        .flat_map(|x| workspaces
                  .iter()
                  .map(|y| (x.to_string(), y.to_string()))
                  .collect::<Vec<(String, String)>>());

    let imgpairs = if repeat {
        imgfiles_collated.split(":").cycle().zip(all_workspaces).filter(|&(i,_)| !i.is_empty()).collect::<Vec<(&str,(String,String))>>()
    } else {
        imgfiles_collated.split(":").zip(all_workspaces).filter(|&(i,_)| !i.is_empty()).collect::<Vec<(&str,(String,String))>>()
    };

    for &(i, (ref m, ref w)) in imgpairs.iter() {
        println!("monitor{}, workspace-{}: {}", m, w, i);
        match xfconf.set_background(&m, &w, i) {
            Err(e) => println!("Failed : {} ", e),
            Ok(_) => ()
        }
    }
}

/// Cycle backdrops. Would cycle all workspaces if the single mode is false
fn do_rotate(xfconf: &XFCEDesktop) {
    if let Err(e) = xfconf.rotate_from_saved() {
        println!("Failed: {}", e);
    }
}

fn do_set_backdrop_mode(xfconf: &mut XFCEDesktop, mode: bool, workspace_num: Option<i64>, rotate: bool) {
    if let Err(e) = match workspace_num {
        Some(n) => xfconf.set_single_workspace_info(mode, if (n < 0) || (n >= (xfconf.workspace_count as i64)) {
            println!("Workspace index ({}) outside valid range [{}..{}]. Not changing it.", n, 0, xfconf.workspace_count);
            None
        } else {
            Some(n)
        }),
        None => xfconf.set_single_workspace_info(mode, None),
    }
    {
        println!("Error setting backdrop mode: {}", e);
    }
    else if rotate {
        if let Err(e) = xfconf.refresh_single_workspace_info() {
            println!("Error refreshing single workspace info: {}", e);
        }
        do_rotate(&xfconf);
    }
}

///
/// Current usage: prog -l <list-file> -q -h -n image-file:image-file:...
///
fn main() {
    let args: Vec<String> = env::args().collect();
    let mut xfconf = XFCEDesktop::new().unwrap();
    let progname = args[0].clone();

    let mut opts = Options::new();

    opts.optflag    ("c", "cycle",    "Cycle backgrounds from list");
    opts.optflag    ("h", "help",     "This help");
    opts.optopt     ("l", "listfile", "Set backdrop list file name", "LISTFILE");
    opts.optflag    ("m", "multiple", "Turn off using single backdrop across all workspaces. Dont use together with '-s'");
    opts.optflagopt ("s", "single",   "Use backdrop from specified workspace for others.", "WORKSPACE");
    opts.optflag    ("q", "query",    "Query the current setting");
    opts.optflag    ("r", "repeat",   "When setting images directly repeat the image file list when not enough images indicated.");

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

    let rotate = matches.opt_present("c");

    if matches.opts_present(&["s".to_string(), "m".to_string(), "l".to_string()]) {
        println!("Use -c to force a backdrop cycle.");
    }

    // Set single workspace mode
    if matches.opt_present("s") || matches.opt_present("m") {
        if matches.opt_present("s") && matches.opt_present("m") {
            println!("Specify one of -s or -m");
            return;
        }

        if matches.opt_present("s") {
            let wsp_idx = matches.opt_str("s").map(|x| x.parse::<i64>().unwrap_or(-1));
            match wsp_idx {
                Some(-1) => println!("Bad workspace index specified : '{}'",
                                     matches.opt_str("s").unwrap_or("?".to_string())),
                _ => do_set_backdrop_mode(&mut xfconf, true, wsp_idx, rotate),
            }
            return;
        }

        if matches.opt_present("m") {
            do_set_backdrop_mode(&mut xfconf, false, None, rotate);
            return;
        }
    }

    // Set list
    if matches.opt_present("l") {
        if let Some(listfile) = matches.opt_str("l") {
            do_setlist(&xfconf, &listfile, rotate);
                return;
        }
        // No list file given
        else {
            print_usage(&progname, opts);
            return;
        }
    }

    // Remaining args as a collection of image:image:... image:image..
    if !matches.free.is_empty() {
        do_setimg(&xfconf, &matches.free.join(":"), matches.opt_present("r"));
    }
    // No args means rotate
    else {
        do_rotate(&xfconf);
    }
}
