extern crate dbus;
extern crate regex;

use std::env;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::error::*;
use std::fmt;

use dbus::{Connection, BusType, Message};
use dbus::arg::{Array, Dict, Variant, RefArg};
use regex::Regex;

const XFCONF_BUS  : &str = "org.xfce.Xfconf";
const XFCONF_PATH : &str = "/org/xfce/Xfconf";
const XFCONF_OBJ  : &str = "org.xfce.Xfconf";
const XFCONF_DESKTOP_CHANNEL : &str = "xfce4-desktop";

struct XFCEDesktop {
    conn: Connection,
    monitors: HashSet<String>,
    _mon_re: Regex,
}

#[derive(Debug)]
enum XFConfError {
    CallError(String),
    DBusError(dbus::Error),
    RegexError(regex::Error),
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

impl Error for XFConfError {
    fn description(&self) -> &str {
        match self {
            &XFConfError::CallError(ref s) => s.as_str(),
            &XFConfError::DBusError(ref dbe) => dbe.message().unwrap_or("<NoMessage>"),
            &XFConfError::RegexError(ref ree) => ree.description(),
            &XFConfError::BadType => "Incorrect type.",
            &XFConfError::NoData => "Message does not have data at that position",
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
            monitors: HashSet::new(),
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
    fn get_monitors(&self) -> Result<HashSet<String>,XFConfError> {
        let m = self.call_method(self.mk_call("GetAllProperties")?.append2(XFCONF_DESKTOP_CHANNEL, "/backdrop/screen0"))?;
        let z: Dict<&str, Variant<Box<RefArg>>, _> = m.get1().unwrap();
        let mons: HashSet<String> = HashSet::from_iter(z.map(|(x,_)| x)
                                                       .map(|fld| self._mon_re.captures(fld))
                                                       .filter(Option::is_some)
                                                       .map(|c| c.unwrap().get(1).unwrap().as_str().to_string()));
        Ok(mons)
    }
}

///
/// Current usage: prog <image-file>
///
fn main() {
    let ag = env::args().nth(1);
    let xfconf = XFCEDesktop::new().unwrap();

    println!("Monitors:");
    for m in xfconf.monitors.iter() {
        println!("{} Img = {}", m, xfconf.get_background(m, "0").unwrap());
        if let &Some(ref img) = &ag {
            println!("Setting {}", &img);
            xfconf.set_background(m, "0", img.as_str()).expect("Failed.");
        }
    }
}
